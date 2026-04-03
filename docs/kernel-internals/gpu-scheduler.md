# GPU Scheduler

> One GPU, many agents. The scheduler makes sure they don't crash each other.

**Source:** [`ore-core/src/scheduler.rs`](../../ore-core/src/scheduler.rs)

---

## Overview

The `GpuScheduler` is a single-permit semaphore-based mutex that ensures only one inference request accesses the GPU at a time. It tracks VRAM state and uses RAII-based `GpuLease` objects to guarantee automatic cleanup - even on panics.

---

## Data Structures

### `GpuScheduler`

```rust
pub struct GpuScheduler {
    execution_lock: Arc<Semaphore>,    // Single-permit semaphore (mutex)
    state: Mutex<GpuState>,           // Tracks what's loaded in VRAM
}

struct GpuState {
    active_model: Option<String>,      // Which model is currently loaded
    active_users: u32,                 // How many concurrent leases exist
}
```

### `GpuLease` (RAII Guard)

```rust
pub struct GpuLease {
    _permit: OwnedSemaphorePermit,    // Holds the semaphore permit
    pub model: String,                // Which model this lease is for
}
```

When a `GpuLease` goes out of scope, Rust's drop semantics automatically release the semaphore permit. This is the same pattern used by `std::sync::MutexGuard` - the GPU is guaranteed to be unlocked even if the inference task panics.

---

## How It Works

### Acquiring a Lease

```rust
pub async fn request_gpu(&self, requested_model: &str) -> GpuLease {
    // 1. Acquire the semaphore (blocks if GPU is busy)
    let permit = Arc::clone(&self.execution_lock)
        .acquire_owned().await.unwrap();

    // 2. Check VRAM state
    let mut state = self.state.lock().await;

    let is_hot_swap = state.active_model.as_ref() == Some(&requested_model.to_string());

    if is_hot_swap {
        // Same model already loaded - share the instance
        state.active_users += 1;
    } else {
        // Different model - evict the old one, load the new one
        state.active_model = Some(requested_model.to_string());
        state.active_users = 1;
    }

    GpuLease { _permit: permit, model: requested_model.to_string() }
}
```

### Request Flow

```
Agent A requests "qwen2.5:0.5b"
     │
     ▼
┌──────────────────────────────────────┐
│ Semaphore.acquire_owned()            │
│ (blocks if permit is held)           │
└───────────────┬──────────────────────┘
                ▼
┌──────────────────────────────────────┐
│ Check GpuState.active_model          │
│                                      │
│  "qwen2.5:0.5b" == "qwen2.5:0.5b"?   │
│                                      │
│  YES → Hot Swap! Skip reload.        │
│         active_users += 1            │
│                                      │
│  NO  → Context Switch.               │
│         Log eviction.                │
│         active_model = new model     │
│         active_users = 1             │
└───────────────┬──────────────────────┘
                ▼
         Return GpuLease
         (inference runs)
                │
                ▼
         GpuLease drops
         → permit released
         → next request unblocks
```

---

## Design Decisions

### Why a Semaphore Instead of a Mutex?

Tokio's `Semaphore` supports `acquire_owned()`, which returns an `OwnedSemaphorePermit` that can be moved into a struct. A regular `Mutex` would require holding the lock for the entire inference duration - `Semaphore` decouples "right to run" from "data access."

### Why RAII?

The `GpuLease` struct holds the `OwnedSemaphorePermit`. When the lease drops:
1. The permit is returned to the semaphore
2. The next queued `acquire_owned()` call unblocks
3. This happens automatically - no manual `.release()` calls, no cleanup code, no risk of deadlocks from error paths

### Why Hot-Swap Detection?

Loading a model into VRAM is expensive (seconds for large GGUF files). If Agent A and Agent B both request `qwen2.5:0.5b`, the second request should share the already-loaded model, not reload it. The scheduler checks `active_model` and increments `active_users` instead of triggering a context switch.

---

## Status Query

```rust
pub async fn get_status(&self) -> String {
    let state = self.state.lock().await;
    match &state.active_model {
        Some(m) => format!("ACTIVE (Model: {}, Users: {})", m, state.active_users),
        None => "IDLE (VRAM Empty)".to_string(),
    }
}
```

Used by `ore top` and the `/health` route to report scheduler state.

---

**← Back to:** [Kernel Internals Index](./README.md)
