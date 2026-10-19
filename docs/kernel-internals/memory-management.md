# Memory Management

> OS-style page files for agent conversation history, physical KV caches, and semantic pipes.

**Source:** [`ore-core/src/memory.rs`](../../ore-core/src/memory.rs) (`Pager`) · Compaction: [`ore-server/src/handlers/inference.rs`](../../ore-server/src/handlers/inference.rs)

---

## Overview

The `Pager` gives agents an operating-system-style page file. There are two tiers:

1. **JSON history:** an agent's chat history, written to SSD after every response and read back before the next one. Conversations survive kernel restarts.
2. **Physical KV cache:** the model's attention key/value tensors, saved as `.safetensors` when the agent loses the engine and restored when it returns, so prefill isn't repeated.

Agents that aren't running don't hold RAM: their state lives in the `memory/` directory of the Ore base directory.

---

## Data Structures

### `ContextMessage`

Defined in `ore-common/src/types.rs` and re-exported by `memory.rs`:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContextMessage {
    pub role: String,       // "user", "assistant", "system"
    pub content: String,    // The message text
}
```

This is the universal message format for every architecture. The driver turns it into the model's chat template at inference time.

---

## Tier 1: JSON History

```rust
pub fn page_out_history(app_id: &str, history: &Vec<ContextMessage>)  // → memory/<app_id>.json
pub fn page_in_history(app_id: &str) -> Vec<ContextMessage>           // empty Vec if none
```

- **Page out** pretty-prints the full history to `memory/<app_id>.json`. The inference handlers call it **after every response**, before the GPU lease is released.
- **Page in** restores it before inference. A missing or unreadable file returns an empty history.

---

## Tier 2: KV-Cache Paging

```rust
pub fn page_out_kv_cache(app_id: &str, model_name: &str,
                         tensors: &HashMap<String, Tensor>, fingerprint: &str)
pub fn page_in_kv_cache(app_id: &str, model_name: &str,
                        device: &Device, current_fingerprint: &str) -> Option<HashMap<String, Tensor>>
pub fn get_history_fingerprint(app_id: &str) -> String
```

### When it happens

KV caches are paged **lazily**, by the native driver, only when an agent loses the engine:

- **Page out:** when a different agent or a different model takes over the engine, the outgoing agent's cache is flattened (`KvManager::flatten_cache`) and written to `memory/<app_id>_<model>.safetensors` in a background task.
- **Page in:** when a `stateful_paging` agent returns and its cache isn't still in RAM, the saved tensors are loaded onto the device and injected into the model (`KvManager::unflatten_cache` → `set_kv_cache`).

If the same agent keeps using the same model, the cache simply stays in RAM ("hot hit") and the SSD isn't touched. See [Native Candle Engine → One Active Model, Many Agents](./native-candle-engine.md#one-active-model-many-agents).

### Fingerprints

Restoring a cache that doesn't match the conversation would corrupt the model's output. So with each cache, the pager writes `memory/<app_id>_<model>.hash`: a SHA-256 over every message's role and content in the JSON history at the time it was saved.

On page-in, the saved hash must equal the fingerprint of the agent's current history. On a mismatch (the history was edited, compacted, or cleared), the cache is ignored (`Context Fingerprint MISMATCH … Forcing Cold Start`) and the model rebuilds its state from the JSON history.

### Measured effect

Resuming an agent on a Ryzen 7 (16 GB RAM, CPU only) took **37.03 s** when prefill was recomputed and **2.41 s** when the saved cache was restored: **15.4× faster**.

---

## Compaction

After each response, the handlers check the agent's `[memory_limits]`:

- **Token cap:** the history's size, estimated with `estimate_tokens_calibrated` and counted exactly with the driver's tokenizer once the estimate reaches 80% of the limit, exceeds `max_json_tokens`.
- **KV cap:** for `stateful_paging` agents, the saved `.safetensors` file exceeds `max_kv_cache_mb`.

If either cap is hit:

| `auto_summarize_on_cap` | What happens |
|---|---|
| `true` (default) | The GPU lease is released and a **background task** asks the model to compress the history into a dense fact summary. The new history is a system message holding the summary, plus the last 2 messages, with a target of 75% of `max_json_tokens`. If the summary is still too long, it's truncated, then the oldest messages are dropped (FIFO). |
| `false` | The oldest messages are dropped (FIFO, keeping the first message and the last 2) until the history fits `max_json_tokens`. |

Either way, the compacted history is paged out, and for `stateful_paging` agents the saved KV cache is deleted (`delete_kv_cache`) and the in-RAM copy invalidated, because the token sequence has changed.

`ore compact <app_id>` (`GET /compact/:app_id`) runs a summarizing compaction on demand.

---

## Semantic Persistence

```rust
pub fn page_out_semantic(pipe_name: &str, chunks: &VecDeque<Arc<MemoryChunk>>)  // → memory/<pipe>.pipe
pub fn page_in_semantic(pipe_name: &str) -> Option<VecDeque<Arc<MemoryChunk>>>
```

When an agent's manifest has `semantic_persistence = true`, a background thread serializes the pipe to a Bincode `.pipe` file after each write, and pipes are restored from disk when needed. See [IPC & Semantic Bus](./ipc-and-semantic-bus.md).

---

## Clearing Memory

```rust
pub fn clear_page(app_id: &str)       // ore clear / GET /clear/:app_id
pub fn delete_kv_cache(app_id: &str)  // used by compaction
```

- `clear_page` removes `<app_id>.json`, `<app_id>.pipe`, and every `<app_id>_*.safetensors` and `<app_id>_*.hash` file.
- `delete_kv_cache` removes only the `.safetensors` / `.hash` files, leaving the JSON history.

---

## Swap File Formats

All files live in `memory/` under the Ore base directory:

| File | Contents |
|---|---|
| `<app_id>.json` | Chat history, human-readable JSON array of `{ role, content }` |
| `<app_id>_<model>.safetensors` | Flattened KV-cache tensors (`:` in the model name becomes `-`) |
| `<app_id>_<model>.hash` | SHA-256 fingerprint of the history the cache belongs to |
| `<pipe_name>.pipe` | Bincode-serialized Semantic Bus pipe |

Example `<app_id>.json`:

```json
[
  { "role": "user", "content": "What is a mutex?" },
  { "role": "assistant", "content": "A mutex (mutual exclusion) is a synchronization primitive..." },
  { "role": "user", "content": "How does it differ from a semaphore?" }
]
```

---

## Manifest Opt-In

```toml
[resources]
json_history = true        # default
stateful_paging = true     # default: false

[memory_limits]
max_json_tokens = 8192
max_kv_cache_mb = 1024
auto_summarize_on_cap = true
```

With `stateful_paging = false`, the KV cache is cleared at the start of every request, and only the JSON history carries context. `stateful_paging = true` requires `json_history = true`: the fingerprint and compaction both depend on the JSON history.

---

## Design Decisions

- **JSON for history.** It's human-readable on purpose (`cat memory/openclaw.json`) and cross-platform.
- **Fingerprinted caches.** A cache is only reused for exactly the conversation it was computed from.
- **Lazy KV paging.** Writing a cache costs I/O proportional to the context length, so it only happens when another agent or model needs the engine.
- **Eager history writes.** History is paged out after every response, so context survives a kernel crash.
- **Synchronous history I/O.** History files are small, so `std::fs` keeps the code path simple. KV-cache writes and semantic persistence run in background tasks.
- **Bounded memory.** `[memory_limits]` caps both the history and the cache, and compaction keeps them inside those caps.

---

**← Back to:** [Kernel Internals Index](./README.md)
