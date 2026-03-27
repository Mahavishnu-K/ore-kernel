# IPC & Semantic Bus

> Agent-to-agent communication and shared vector memory.

**Source:** [`ore-core/src/ipc.rs`](../../ore-core/src/ipc.rs)

---

## Overview

ORE's IPC layer has three components:

| Component | Purpose | Data Structure |
|---|---|---|
| **Message Bus** | Real-time agent-to-agent messaging | `DashMap<String, broadcast::Sender>` |
| **Semantic Bus** | In-memory vector database for shared knowledge | `DashMap<String, Vec<MemoryChunk>>` |
| **Rate Limiter** | Per-agent token quota enforcement | `DashMap<String, (u32, Instant)>` |

All three use `DashMap` for lock-free concurrent access across async tasks.

---

## Message Bus (Tier 1: Direct Messaging)

```rust
pub struct MessageBus {
    channel: DashMap<String, broadcast::Sender<AgentMessage>>,
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

**Register a listener** — An agent subscribes to its own channel:

```rust
pub fn register_listener(&self, app_id: &str) -> broadcast::Receiver<AgentMessage> {
    let sender = self.channel.entry(app_id.to_string()).or_insert_with(|| {
        let (tx, _) = broadcast::channel(100);
        tx
    });
    sender.subscribe()
}
```

**Send a message** — Agent A sends to Agent B's channel:

```rust
pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
    if let Some(target_channel) = self.channel.get(&msg.to_app) {
        let _ = target_channel.send(msg);
        Ok(())
    } else {
        Err(format!("Agent '{}' is not currently listening.", msg.to_app))
    }
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
    memory_pipes: DashMap<String, Vec<MemoryChunk>>,    // Named data pipes
    embedding_cache: DashMap<u64, (Vec<f32>, u64)>,     // Hash → (vector, timestamp)
    cache_ttl_secs: u64,
    pipe_ttl_secs: u64,
}
```

### `MemoryChunk`

```rust
pub struct MemoryChunk {
    pub text: String,           // Original text
    pub vector: Vec<f32>,       // Embedding vector
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
    self.embedding_cache.insert(hash, (vector.clone(), timestamp));

    // 2. Append the chunk to the named pipe
    let mut pipe = self.memory_pipes.entry(pipe_name.to_string()).or_default();
    pipe.push(MemoryChunk { text, vector, source_app: source_app.to_string(), timestamp });
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
) -> Vec<String>
```

The search algorithm:

1. **Iterate** all chunks in the specified pipe
2. **Filter** by `source_app` (if `filter_app` is provided)
3. **Score** each chunk:
   - **Cosine similarity** between the query vector and chunk vector
   - **Time decay** — Older memories lose 1% relevance per hour (clamped at 50% minimum): `decay = (1.0 - hours_old * 0.01).clamp(0.5, 1.0)`
   - **Final score** = `cosine_similarity × decay_factor`
4. **Sort** descending by final score
5. **Return** top-K text chunks

### Cosine Similarity

```rust
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
}
```

### Embedding Cache

The cache maps `hash(text) → (embedding_vector, timestamp)`:

```rust
pub fn get_cached_embedding(&self, text: &str) -> Option<Vec<f32>> {
    let hash = Self::hash_text(text);
    self.embedding_cache.get(&hash).map(|v| v.0.clone())
}
```

If the same text is written twice, the embedding is served from cache instead of re-invoking the embedder. The hash uses Rust's `DefaultHasher` for speed.

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
    // 1. Sweep the embedding cache — evict entries older than cache_ttl_secs
    self.embedding_cache.retain(|_, (_, timestamp)| {
        current_time.saturating_sub(*timestamp) < self.cache_ttl_secs
    });

    // 2. Sweep each pipe — evict chunks older than pipe_ttl_secs
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

- **`DashMap` everywhere** — All three components need concurrent access from multiple async tasks. `DashMap` provides lock-free read/write without wrapping everything in `Arc<Mutex<HashMap>>`, reducing contention.
- **`broadcast` not `mpsc`** — The Message Bus uses `broadcast` so multiple listeners can subscribe to the same channel. This enables future multi-consumer patterns (e.g., logging agents observing conversations).
- **Time decay, not FIFO** — Search results favor recent memories with a 1%/hour decay. This naturally surfaces fresh knowledge without explicit "forget" operations, while clamping at 50% ensures old memories aren't completely lost.
- **Hash-based embedding cache** — Uses `DefaultHasher` (not cryptographic) for speed. The cache is a performance optimization, not a security boundary — hash collisions would cause a cache hit on different text, which is harmless.

---

**← Back to:** [Kernel Internals Index](./README.md)
