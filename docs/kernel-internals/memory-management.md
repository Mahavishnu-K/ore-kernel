# Memory Management

> OS-style page files for agent conversation history.

**Source:** [`ore-core/src/memory.rs`](../../ore-core/src/memory.rs)

---

## Overview

The `Pager` provides an operating system-style page file mechanism for agent conversation context. When an agent finishes an inference request, its chat history is serialized to JSON on the SSD. On the next request, the history is restored from disk, enabling multi-turn conversations across kernel restarts.

This mirrors how an OS pages idle processes to disk - agents that aren't actively running don't consume RAM.

---

## Data Structures

### `ContextMessage`

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContextMessage {
    pub role: String,       // "user", "assistant", "system"
    pub content: String,    // The message text
}
```

This is the universal message format used across **all** model architectures (Llama, Qwen, etc.). The driver converts it to architecture-specific chat templates at inference time.

---

## Operations

### Page Out (RAM → SSD)

```rust
pub fn page_out_history(app_id: &str, history: &Vec<ContextMessage>) {
    Self::ensure_swap_drive();
    let path = format!("{}/{}.json", Self::SWAP_DIR, app_id);

    if let Ok(data) = serde_json::to_string_pretty(history) {
        let _ = fs::write(&path, data);
    }
}
```

Serializes the agent's full chat history to `memory/<app_id>.json` as pretty-printed JSON. Called **after** every inference response.

### Page In (SSD → RAM)

```rust
pub fn page_in_history(app_id: &str) -> Vec<ContextMessage> {
    let path = format!("{}/{}.json", Self::SWAP_DIR, app_id);

    if Path::new(&path).exists()
        && let Ok(data) = fs::read_to_string(&path)
        && let Ok(history) = serde_json::from_str::<Vec<ContextMessage>>(&data)
    {
        return history;
    }
    Vec::new()
}
```

Restores frozen context from disk. Called **before** inference to reconstruct the conversation. Returns an empty `Vec` if no swap file exists.

### KV-Cache Paging (RAM ↔ SSD)

The Pager also handles the physical AI memory state—the Attention Key-Value (KV) Cache tensors.

```rust
pub fn page_out_kv_cache(app_id: &str, model_name: &str, tensors: &HashMap<String, Tensor>) {
    // Saves raw math matrices to memory/<app_id>_<model_name>.safetensors
}

pub fn page_in_kv_cache(app_id: &str, model_name: &str, device: &Device) -> Option<HashMap<String, Tensor>> {
    // Loads the tensors back directly into the GPU/CPU memory
}
```

This prevents the LLM from having to re-process thousands of tokens to rebuild its internal state when an agent is brought back from disk.

### Clear Page

```rust
pub fn clear_page(app_id: &str) {
    let _ = fs::remove_file(format!("{}/{}.json", Self::SWAP_DIR, app_id));
    let _ = fs::remove_file(format!("{}/{}.pipe", Self::SWAP_DIR, app_id));

    // Sweep for any Model-Specific Safetensor KV-Caches
    if let Ok(entries) = fs::read_dir(Self::SWAP_DIR) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with(&format!("{}_", app_id)) && file_name.ends_with(".safetensors") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
```

Wipes an agent's frozen memory. Called via `ore clear <app_id>` or `GET /clear/:app_id`. Removes `.json` fallback files, `.pipe` semantic memory files, and cleans up any model-specific `.safetensors` KV-Caches.

### Semantic Persistence (Page Out / Page In)

The pager now supports freezing entire `SemanticBus` vector pipelines directly to disk using Bincode serialization.

```rust
pub fn page_out_semantic(pipe_name: &str, chunks: &VecDeque<Arc<MemoryChunk>>) {
    // ...
    if let Ok(data) = bincode::serialize(chunks) {
        let _ = fs::write(&path, data);
    }
}

pub fn page_in_semantic(pipe_name: &str) -> Option<VecDeque<Arc<MemoryChunk>>> {
    // Reads Bincode binary and restores the VecDeque to RAM
}
```

When an agent's manifest has `semantic_persistence = true`, the kernel spawns a background thread to automatically serialize the pipe to a `.pipe` file upon any knowledge ingestion.

---

## Swap File Formats

The pager generates three types of files in the `memory/` directory:

### 1. JSON History (`<app_id>.json`)
Stores the raw conversation thread. Human-readable and cross-platform:

```json
[
  {
    "role": "user",
    "content": "What is a mutex?"
  },
  {
    "role": "assistant",
    "content": "A mutex (mutual exclusion) is a synchronization primitive..."
  },
  {
    "role": "user",
    "content": "How does it differ from a semaphore?"
  }
]
```

### 2. Semantic Pipes (`<pipe_name>.pipe`)
Binary bincode representations of an agent's `SemanticBus` vectors.

### 3. Safetensors KV-Cache (`<app_id>_<model>.safetensors`)
Physical snapshot of the AI model's internal memory state (`HashMap<String, Tensor>`). Used to skip prompt re-processing upon wake-up.

---

## Manifest Opt-In

Agents must explicitly enable SSD paging in their manifest:

```toml
[resources]
json_history = true
stateful_paging = true
```

When `stateful_paging = false` (default), the handler skips the page-in/page-out calls - the agent starts every request with a clean context. Note that `stateful_paging` requires `json_history = true` to prevent KV-cache corruption during memory compaction.

---

## Design Decisions

- **JSON, not binary (for chat history)** - Swap files are human-readable on purpose. This makes debugging agent memory trivial (`cat memory/openclaw.json`) and keeps the format cross-platform.
- **Bincode for Semantic Pipes** - Semantic memory vectors are written as `.pipe` files using Bincode, as Bincode freezes the RAM structure into pure 1s and 0s instantly, allowing high-performance mapping.
- **Synchronous I/O** - The pager uses `std::fs` (sync) rather than `tokio::fs` (async) for history. Swap files are small (kilobytes), and adding async here would complicate the code path for negligible latency savings. (Semantic persistence runs in a background thread).
- **Eager writes** - History is paged out after every response, not batched. This means agent context survives even if the kernel crashes unexpectedly.
- **Memory Limits Compaction** - Swap files are kept in check by the `memory_limits` configuration in the `AppManifest` (`max_json_tokens` and `max_kv_cache_mb`), which trigger automatic summarization when boundaries are hit.

---

**← Back to:** [Kernel Internals Index](./README.md)
