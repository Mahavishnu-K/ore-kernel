# SSD Pager

> OS-style page files for agent conversation history.

**Source:** [`ore-core/src/swap.rs`](../../ore-core/src/swap.rs)

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

Serializes the agent's full chat history to `swap/<app_id>.json` as pretty-printed JSON. Called **after** every inference response.

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

### Clear Page

```rust
pub fn clear_page(app_id: &str) {
    let path_json = format!("{}/{}.json", Self::SWAP_DIR, app_id);
    let path_bin = format!("{}/{}.bin", Self::SWAP_DIR, app_id);

    let _ = fs::remove_file(path_json);
    let _ = fs::remove_file(path_bin);
}
```

Wipes an agent's frozen memory. Called via `ore clear <app_id>` or `GET /clear/:app_id`. Removes both `.json` and `.bin` files (the `.bin` format is reserved for future binary swap formats).

---

## Swap File Format

Each agent's swap file is stored at `swap/<app_id>.json`:

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

---

## Manifest Opt-In

Agents must explicitly enable SSD paging in their manifest:

```toml
[resources]
stateful_paging = true
```

When `stateful_paging = false` (default), the handler skips the page-in/page-out calls - the agent starts every request with a clean context.

---

## Design Decisions

- **JSON, not binary** - Swap files are human-readable on purpose. This makes debugging agent memory trivial (`cat swap/openclaw.json`) and keeps the format cross-platform.
- **Synchronous I/O** - The pager uses `std::fs` (sync) rather than `tokio::fs` (async). Swap files are small (kilobytes), and adding async here would complicate the code path for negligible latency savings.
- **Eager writes** - History is paged out after every response, not batched. This means agent context survives even if the kernel crashes unexpectedly.
- **No size limits (yet)** - Swap files grow unbounded. Future versions will add a configurable max history depth to prevent disk bloat from long-lived agents.

---

**← Back to:** [Kernel Internals Index](./README.md)
