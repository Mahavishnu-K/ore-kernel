# IPC & Semantic Bus

> Agent-to-agent communication and shared vector memory.

**Source:** [`ore-core/src/ipc.rs`](../../ore-core/src/ipc.rs)

---

## Overview

ORE's IPC layer has three components:

| Component | Purpose | Data Structure |
|---|---|---|
| **Message Bus** | Real-time agent-to-agent messaging | `DashMap<String, (mpsc::Tx, Mutex<mpsc::Rx>)>` |
| **Semantic Bus** | In-memory vector database for shared knowledge | `DashMap<String, VecDeque<Arc<MemoryChunk>>>` |
| **Rate Limiter** | Per-agent token quota enforcement | `DashMap<String, (u32, Instant)>` |

All three use `DashMap` for lock-free concurrent access across async tasks.

---

## Message Bus (Tier 1: Direct Messaging)

```rust
pub struct MessageBus {
    channel: DashMap<String, (mpsc::UnboundedSender<AgentMessage>, StdMutex<mpsc::UnboundedReceiver<AgentMessage>>)>,
}
```

### `AgentMessage`

```rust
pub struct AgentMessage {
    pub from_app: String,
    pub to_app: String,
    pub payload: String,
    pub timestamp: u64,
}
```

### Operations

**Read a message** - An agent polls for messages on its own channel:

```rust
pub fn read_message(&self, app_id: &str) -> Option<AgentMessage> {
    if let Some(target_channel) = self.channel.get(app_id) {
        let mut rx = target_channel.1.lock().unwrap();
        // Non-blocking read
        if let Ok(msg) = rx.try_recv() {
            return Some(msg);
        }
    }
    None
}
```

**Send a message** - Agent A sends to Agent B's channel:

```rust
pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
    let target_channel = self.channel.entry(msg.to_app.clone()).or_insert_with(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        (tx, StdMutex::new(rx))
    });

    target_channel.0.send(msg).map_err(|_| "Failed to deliver message.".to_string())
}
```

### Permission Check

The sender's manifest must list the receiver in `allowed_agent_targets`:

```toml
[ipc]
allowed_agent_targets = ["writer_agent"]
```

This check is enforced in the handler layer (`ore-server/src/handlers/ipc.rs`), not in the `MessageBus` itself.

---

## Semantic Bus (Tier 2: Vector Memory)

An in-memory vector database that enables agents to share knowledge through natural-language search.

```rust
pub struct SemanticBus {
    memory_pipes: DashMap<String, VecDeque<Arc<MemoryChunk>>>,    // Named data pipes
    embedding_cache: DashMap<u64, (Arc<Vec<f32>>, u64)>,          // Hash → (vector, timestamp)
    cache_ttl_secs: u64,
    pipe_ttl_secs: u64,
}
```

### `MemoryChunk`

```rust
pub struct MemoryChunk {
    pub text: Arc<String>,      // Original text
    pub vector: Arc<Vec<f32>>,  // Pre-normalized embedding vector
    pub source_app: String,     // Which agent wrote this
    pub timestamp: u64,         // Unix timestamp (epoch seconds)
}
```

### Writing Knowledge

```rust
pub fn write_chunk(&self, pipe_name: &str, text: String, vector: Vec<f32>, source_app: &str) {
    // 1. Cache the embedding (hash → vector) for deduplication
    let hash = Self::hash_text(&text);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    self.embedding_cache.insert(hash, (Arc::new(vector.clone()), timestamp));

    // 2. Append the chunk to the named pipe
    let mut pipe = self.memory_pipes.entry(pipe_name.to_string()).or_default();
    // ... constructs MemoryChunk and pushes to pipe
}

/// Bypasses cache insertion to directly write an existing cached pointer to a new pipe
pub fn write_cached_chunk(&self, pipe_name: &str, text: String, arc_vector: Arc<Vec<f32>>, source_app: &str) {
    // ZERO COPY - We just pass the memory pointer!
    // ... constructs MemoryChunk using the Arc pointer and pushes to pipe
}
```

Before writing, the handler layer splits the raw text into sliding window chunks:

```rust
pub fn create_sliding_windows(text: &str, window_size: usize, overlap: usize) -> Vec<String> {
    // Splits by whitespace, slides forward by (window_size - overlap) words
    // Maintains context coherence across chunk boundaries
}
```

Default: 50 words per chunk, 10 words overlap.

### Searching

```rust
pub fn search_pipe(
    &self,
    pipe_name: &str,
    query_vector: &[f32],
    top_k: usize,
    filter_app: Option<&str>,
) -> Vec<(f32, Arc<MemoryChunk>)>
```

The search algorithm:

1. **Iterate** all chunks in the specified pipe
2. **Filter** by `source_app` (if `filter_app` is provided)
3. **Score** each chunk:
   - **Fast Dot Product** between the query vector and chunk vector (since vectors are pre-normalized)
   - **Time decay** - Older memories lose 1% relevance per hour (clamped at 50% minimum): `decay = (1.0 - hours_old * 0.01).clamp(0.5, 1.0)`
   - **Final score** = `dot_product × decay_factor`
4. **BinaryHeap (Top-K)** - Push into a max-heap of size K (`O(log K)` complexity) instead of sorting the whole pipe `O(N log N)`.
5. **Return** top-K chunks sorted by score.

### Fast Dot Product (Normalized Cosine Similarity)

```rust
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() { return 0.0; }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
```

### Embedding Cache

The cache maps `hash(text) → (embedding_vector, timestamp)`:

```rust
pub fn get_cached_embedding(&self, text: &str) -> Option<Arc<Vec<f32>>> {
    let hash = Self::hash_text(text);
    if let Some(entry) = self.embedding_cache.get(&hash) {
        // ... dynamically checks TTL
        return Some(entry.0.clone());
    }
    None
}
```

If the same text is written twice, the embedding is served directly from the cache buffer instead of re-invoking the embedder. Fast IPC caching utilizes `write_cached_chunk()` allowing identical embedding nodes to share the same `Arc<Vec<f32>>` pointer across countless memory chunks without duplicating bytes in RAM. For pure querying (which shouldn't be added to a pipe), the handlers call `cache_only()` to ensure the generated embedding query is cached for speed without adding it to the readable `VecDeque` memory space.

### Pipe Permissions

Both read and write operations are gated by the manifest:

```toml
[ipc]
allowed_semantic_pipes = ["rust_docs", "research_papers"]
```

An agent can only access pipes that are explicitly listed. This prevents unauthorized cross-agent memory access.

---

## Garbage Collection

The kernel runs a background task that wakes every hour and sweeps stale data:

```rust
pub fn run_garbage_collection(&self) {
    // 1. Sweep the embedding cache - evict entries older than cache_ttl_secs
    self.embedding_cache.retain(|_, (_, timestamp)| {
        current_time.saturating_sub(*timestamp) < self.cache_ttl_secs
    });

    // 2. Sweep each pipe - evict chunks older than pipe_ttl_secs
    for mut pipe_ref in self.memory_pipes.iter_mut() {
        pipe_contents.retain(|chunk| {
            current_time.saturating_sub(chunk.timestamp) < self.pipe_ttl_secs
        });
    }

    // 3. Prune empty pipes
    self.memory_pipes.retain(|_, chunks| !chunks.is_empty());
}
```

TTLs are configured in `ore.toml`:

```toml
[memory]
cache_ttl_hours = 24    # Embedding cache lifetime
pipe_ttl_hours = 32     # Semantic pipe data lifetime
```

Setting either to `0` disables GC for that category (infinite retention).

---

## Rate Limiter

```rust
pub struct RateLimiter {
    usage: DashMap<String, (u32, Instant)>,  // app_id → (tokens_used, window_start)
}
```

### Algorithm

```rust
pub fn check_and_add(&self, app_id: &str, limit: u32, requested_tokens: u32) -> bool {
    let mut entry = self.usage.entry(app_id.to_string()).or_insert((0, Instant::now()));

    // Reset counter if 60 seconds have elapsed
    if entry.1.elapsed() > Duration::from_secs(60) {
        entry.0 = 0;
        entry.1 = Instant::now();
    }

    // Check quota
    if entry.0 + requested_tokens > limit {
        return false;  // Blocked!
    }

    entry.0 += requested_tokens;
    true
}
```

The quota comes from the agent's manifest: `[resources].max_tokens_per_minute`.

---

## Design Decisions

- **`DashMap` everywhere** - All three components need concurrent access from multiple async tasks. `DashMap` provides lock-free read/write without wrapping everything in `Arc<Mutex<HashMap>>`, reducing contention.
- **Vectors are Zero-Copied** - Using `Arc<Vec<f32>>` and `Arc<MemoryChunk>` ensures that large knowledge documents don't duplicate memory during pipe transitions and memory queries. 
- **Time decay, not FIFO** - Search results favor recent memories with a 1%/hour decay. This naturally surfaces fresh knowledge without explicit "forget" operations, while clamping at 50% ensures old memories aren't completely lost.
- **Fast Dot-Product replacing Cosine Similarity** - By strictly normalizing embed tensors at insertion point, the expensive search-time `sqrt()` normalization is removed, resulting in purely arithmetic inner `dot_product` mappings. Max-heap (`BinaryHeap`) guarantees immediate O(log K) extraction.
- **`mpsc` instead of `broadcast`** - The Message Bus transitioned to `mpsc::unbounded_channel` to allow agent-only non-blocking delivery avoiding broadcast lag.

---

**← Back to:** [Kernel Internals Index](./README.md)
