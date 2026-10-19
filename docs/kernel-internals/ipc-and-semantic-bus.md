# IPC & Semantic Bus

> Agent-to-agent communication and shared vector memory.

**Source:** [`ore-core/src/ipc.rs`](../../ore-core/src/ipc.rs) · Handlers: [`ore-server/src/handlers/ipc.rs`](../../ore-server/src/handlers/ipc.rs)

---

## Overview

Ore's IPC layer has three components:

| Component | Purpose | Data Structure |
|---|---|---|
| **Message Bus** | Agent-to-agent messaging | `DashMap<String, (mpsc::Sender, Mutex<mpsc::Receiver>)>`, bounded |
| **Semantic Bus** | In-memory vector database for shared knowledge | `DashMap<String, PipeState>` + an embedding cache |
| **Rate Limiter** | Per-agent token quota | `DashMap<String, (u32, Instant)>` |

All three use `DashMap` for sharded, low-contention concurrent access across async tasks.

---

## Message Bus (Tier 1: Direct Messaging)

```rust
pub const DEFAULT_MESSAGE_BACKLOG: usize = 1024;

pub struct MessageBus {
    channel: DashMap<String, (mpsc::Sender<AgentMessage>, StdMutex<mpsc::Receiver<AgentMessage>>)>,
    backlog_capacity: usize,
}

pub struct AgentMessage {
    pub from_app: String,
    pub to_app: String,
    pub payload: String,
    pub timestamp: u64,
}
```

### Operations

**Send:** the receiver's inbox is created on first use as a **bounded** channel (`DEFAULT_MESSAGE_BACKLOG = 1024` messages; `MessageBus::with_capacity` sets another size, minimum 1). Sending never blocks:

```rust
pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
    let target_channel = self.channel.entry(msg.to_app.clone()).or_insert_with(|| {
        let (tx, rx) = mpsc::channel(self.backlog_capacity);
        (tx, StdMutex::new(rx))
    });

    target_channel.0.try_send(msg).map_err(|e| match e {
        TrySendError::Full(dropped) => format!(
            "Target agent '{}' queue is full (capacity: {}). Backpressure applied.",
            dropped.to_app, self.backlog_capacity
        ),
        TrySendError::Closed(_) => "Target agent channel receiver has closed.".to_string(),
    })
}
```

A full inbox **rejects** the new message instead of growing without bound, so an agent that never reads its messages can't exhaust the kernel's memory.

**Read:** an agent polls its own inbox without blocking (`try_recv`). `GET /ipc/listen/:app_id` returns the next message, or `null`.

### Permission Check

The sender's manifest must list the receiver in `allowed_agent_targets`:

```toml
[ipc]
allowed_agent_targets = ["writer_agent"]
```

This check is enforced in the handler layer (`ore-server/src/handlers/ipc.rs`), not in `MessageBus` itself.

---

## Semantic Bus (Tier 2: Vector Memory)

An in-memory vector database that lets agents share knowledge through natural-language search.

```rust
pub struct SemanticBus {
    memory_pipes: DashMap<String, PipeState>,
    embedding_cache: DashMap<u64, (Arc<Vec<f32>>, u64)>, // hash(text) → (vector, timestamp)
    cache_ttl_secs: u64,
    pipe_ttl_secs: u64,
}

pub enum PipeState {
    Active { is_persistent: bool, chunks: VecDeque<Arc<MemoryChunk>> },
    PagedToDisk,   // persistent pipe evicted from RAM; lives in memory/<pipe>.pipe
}

pub struct MemoryChunk {
    pub text: Arc<String>,
    pub vector: Arc<Vec<f32>>, // pre-normalized unit vector
    pub source_app: String,
    pub timestamp: u64,        // Unix seconds
}
```

### Writing Knowledge (`POST /ipc/share`)

1. The handler checks `allowed_semantic_pipes`.
2. The text is split with the requested **chunking strategy** (`SemanticBus::chunk_text`):

   | Strategy | Behavior |
   |---|---|
   | `sliding_window` (default) | Fixed-size word windows that advance by `size - overlap` |
   | `sentence_aware` | Packs whole sentences into chunks of up to `size`, with overlap |
   | `paragraph` | Splits on paragraphs, keeping each under `size` |
   | `exact_match` | The whole text as one chunk |

   Defaults: `chunk_size = 100`, `chunk_overlap = 20` (an overlap ≥ the size is replaced with `size / 4`).
3. Chunks already in the embedding cache reuse their vector. The rest are embedded in one batch, under the `embedder_lock` so only one embedder is loaded at a time.
4. Each chunk is written to the pipe:
   - `write_chunk` **normalizes the vector to unit length**, stores it in the cache, and appends the chunk.
   - `write_cached_chunk` appends a chunk that shares an existing `Arc<Vec<f32>>`, with no copy and no re-embedding.

Each pipe holds at most **10,000 chunks**. Beyond that, the oldest chunk is dropped.

### Searching (`POST /ipc/search`)

```rust
pub fn search_pipe(
    &self,
    pipe_name: &str,
    query_vector: &[f32],
    top_k: usize,
    filter_app: Option<&str>,
    allow_time_decay: bool,
) -> Vec<(f32, Arc<MemoryChunk>)>
```

1. **Page-fault recovery:** if the pipe isn't in RAM, or is `PagedToDisk`, it's restored from `memory/<pipe>.pipe` first.
2. **Filter** by `source_app` if `filter_app` is set.
3. **Score** each chunk with a dot product against the query vector. Both are unit vectors, so this equals cosine similarity.
4. **Time decay** (if the searching agent has `allow_time_decay = true`): `score × (1 − 0.01 × hours_old)`, clamped to `[0.5, 1.0]`. Memories lose 1% relevance per hour, down to half.
5. **Top-K** with a bounded `BinaryHeap`: `O(N log K)` instead of sorting the whole pipe.
6. Results are returned highest score first (default `top_k = 5`).

### Fast Dot Product

Because vectors are normalized when they're written, search never needs `sqrt`. The dot product uses 8 independent accumulators over `chunks_exact(8)`, which breaks the floating-point dependency chain and lets the compiler auto-vectorize:

```rust
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() { return 0.0; }
    // sum0..sum7 accumulate lanes 0..7 of each 8-wide chunk,
    // then the remainder is added at the end
    ...
}
```

### Embedding Cache

The cache maps `hash(text) → (vector, timestamp)`. If the same text is shared again, its embedding comes from the cache, and identical chunks share one `Arc<Vec<f32>>` across pipes. Search queries are cached with `cache_only()`, which caches the vector without adding it to any pipe.

### Pipe Permissions

Both reads and writes are gated by the manifest:

```toml
[ipc]
allowed_semantic_pipes = ["rust_docs", "research_papers"]
```

An agent can only access pipes that are explicitly listed.

### Semantic Persistence

```toml
[ipc]
semantic_persistence = true   # default
allow_time_decay = false
```

With `semantic_persistence = true`, the handler spawns a background task after each `/ipc/share` that writes the pipe to `memory/<pipe_name>.pipe` (Bincode) via `Pager::page_out_semantic`. Pipes survive kernel restarts and are paged back in on first use. With `false`, the pipe lives only in RAM.

---

## Garbage Collection

A background loop in `ore-server` calls `run_garbage_collection()` every hour (see [Configuration](../configuration.md#memory-section)):

1. **Embedding cache:** entries older than `cache_ttl_hours` are evicted.
2. **Persistent pipes:** written to SSD and switched to `PagedToDisk`, which frees their RAM without losing data. The next read or write pages them back in.
3. **Ephemeral pipes:** chunks older than `pipe_ttl_hours` are evicted. Pipes that end up empty are removed.

A TTL of `0` disables that sweep (infinite retention in RAM).

```toml
[memory]
cache_ttl_hours = 24
pipe_ttl_hours = 32
```

---

## Rate Limiter

```rust
pub struct RateLimiter {
    usage: DashMap<String, (u32, Instant)>,  // app_id → (tokens_used, window_start)
}

pub fn check_and_add(&self, app_id: &str, limit: u32, requested_tokens: u32) -> bool {
    let mut entry = self.usage.entry(app_id.to_string()).or_insert((0, Instant::now()));
    if entry.1.elapsed() > Duration::from_secs(60) {   // new 60-second window
        entry.0 = 0;
        entry.1 = Instant::now();
    }
    if entry.0 + requested_tokens > limit {
        return false;                                   // rejected (429)
    }
    entry.0 += requested_tokens;
    true
}
```

The limit is the agent's `[resources].max_tokens_per_minute`. `/run` currently charges a flat 1,000 tokens per request.

---

## Tests

`ore-core/src/ipc.rs` has unit tests for the unrolled dot product, bounded Message Bus backpressure, and sentence-aware chunking (`cargo test -p ore-core ipc`).

---

## Design Decisions

- **`DashMap` everywhere.** All three components are accessed concurrently from many async tasks; sharded locking avoids one global `Mutex<HashMap>`.
- **Bounded inboxes.** Backpressure is explicit: senders get an error instead of the kernel buffering unboundedly.
- **Zero-copy vectors.** `Arc<Vec<f32>>` and `Arc<MemoryChunk>` let pipes, the cache, and search results share memory instead of duplicating it.
- **Normalize once, at write time.** Search is a pure dot product, with no per-query `sqrt`.
- **Page persistent pipes, don't drop them.** `PipeState::PagedToDisk` makes GC safe for persistent memory; a page fault restores the pipe transparently.
- **Time decay, not deletion.** Fresh knowledge ranks higher without an explicit "forget" step, and the 50% floor keeps old memories reachable.

---

**← Back to:** [Kernel Internals Index](./README.md)
