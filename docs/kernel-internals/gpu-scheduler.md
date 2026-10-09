# GPU Scheduler & Multi-Tenancy

> Multi-tenant VRAM bin-packing, physical memory accounting, dynamic KV-cache estimation, and LRU eviction.

**Source:** [`ore-core/src/scheduler.rs`](../../ore-core/src/scheduler.rs)

---

## Overview

Modern multi-agent workflows quickly run into the **VRAM Wall**. A single inference engine typically locks the entire GPU, causing Out-Of-Memory (OOM) panics or serialized bottlenecks when concurrent agents invoke different models.

ORE's `GpuScheduler` is an intelligent, multi-tenant hypervisor for local inference memory. Instead of a naive single-model mutex, it implements:
1. **Multi-Model Co-Residency**: Multiple models can co-exist inside GPU VRAM simultaneously as long as physical memory permits.
2. **Physical VRAM Accounting (`MemoryAccountant`)**: Tracks live free memory, hardware headroom, and safety margins (default: 512 MB).
3. **Exact KV-Cache Dimensioning**: Mathematically reads GGUF architecture metadata at runtime to compute precise per-token KV memory demands before admission.
4. **LRU Eviction (Least Recently Used)**: Gracefully unloads idle models only when memory pressure demands it, maximizing cache hits.
5. **RAII-Based `GpuLease`**: Uses Tokio semaphore permits (32 concurrent handles) with automatic drop-based memory cleanup.

---

## Data Structures

### `GpuScheduler`

```rust
pub struct GpuScheduler {
    execution_lock: Arc<Semaphore>,       // 32-permit execution semaphore
    state: Arc<Mutex<GpuState>>,          // Multi-tenant GPU state
    driver: Arc<dyn InferenceDriver>,     // HAL driver for model load/unload
    config: SchedulerConfig,              // Model-specific overrides
}

struct GpuState {
    registry: ModelRegistry,              // Active and loaded model records
    memory_provider: Box<dyn GpuMemoryProvider>, // NVML or system memory provider
    accountant: MemoryAccountant,         // Tracks reserved budgets & safety buffers
    active_app_id: Option<String>,        // Currently active agent context
}
```

### `GpuMemoryProvider` Trait

Decouples physical memory querying from the host platform:

```rust
pub trait GpuMemoryProvider: Send + Sync {
    fn total_vram_mb(&self) -> u64;
    fn used_vram_mb(&self) -> u64;
    fn free_vram_mb(&self) -> u64;
}
```

- **`NvmlGpuMemoryProvider`**: Leverages NVIDIA Management Library (NVML) to query exact hardware VRAM metrics on dedicated GPUs.
- **`SystemMemoryProvider`**: Universal fallback powered by `sysinfo` for unified memory architectures (Apple Metal, Intel Iris, AMD APUs, and CPU-only hosts).

### `MemoryAccountant`

Guarantees safety margins and tracks VRAM reservations:

```rust
pub struct MemoryAccountant {
    pub reserved_vram_mb: u64,
    pub safety_margin_mb: u64, // Default: 512 MB safety buffer
}

impl MemoryAccountant {
    pub fn can_admit(&self, memory_provider: &dyn GpuMemoryProvider, required_vram_mb: u64) -> bool {
        let available = memory_provider
            .free_vram_mb()
            .saturating_sub(self.reserved_vram_mb);
        available >= required_vram_mb + self.safety_margin_mb
    }

    pub fn reserve(&mut self, amount_mb: u64) {
        self.reserved_vram_mb += amount_mb;
    }

    pub fn release_reservation(&mut self, amount_mb: u64) {
        self.reserved_vram_mb = self.reserved_vram_mb.saturating_sub(amount_mb);
    }
}
```

### `ModelRegistry` & `LoadedModel`

Tracks state, usage timestamps, and active in-flight leases:

```rust
pub struct ModelRegistry {
    pub models: HashMap<String, LoadedModel>,
}

pub struct LoadedModel {
    pub model_id: String,
    pub estimated_vram_mb: u64,
    pub observed_load_delta_mb: Option<u64>,
    pub active_requests: usize,
    pub last_used: Instant,
    pub status: ModelStatus, // Loading, Loaded, Unloading, Failed
}
```

### `GpuLease` (RAII Guard)

```rust
pub struct GpuLease {
    _permit: OwnedSemaphorePermit,
    pub model: String,
    state: Arc<Mutex<GpuState>>,
    reserved_vram_mb: u64,
}

impl Drop for GpuLease {
    fn drop(&mut self) {
        let state = Arc::clone(&self.state);
        let model_id = self.model.clone();
        let reserved_mb = self.reserved_vram_mb;

        tokio::spawn(async move {
            let mut state = state.lock().await;
            if let Some(model) = state.registry.models.get_mut(&model_id) {
                model.active_requests = model.active_requests.saturating_sub(1);
                model.last_used = Instant::now();
            }
            state.accountant.release_reservation(reserved_mb);
        });
    }
}
```

---

## The Physics: Exact KV-Cache Dimensioning

Instead of guessing memory requirements, `GpuScheduler` inspects the GGUF model header directly using `candle_core::quantized::gguf_file`:

1. **Exact Weight Size**: Read from file metadata on disk.
2. **Architecture Geometry**:
   - `layers`: Extracted from `{arch}.block_count` (e.g., 32 layers)
   - `kv_heads`: Extracted from `{arch}.attention.head_count_kv` (e.g., 8 heads)
   - `head_dim`: Extracted from `{arch}.attention.key_length` or `{arch}.embedding_length / head_count` (e.g., 128)
3. **Mathematical Formula**:
   $$\text{Bytes per Token} = 2 \times \text{layers} \times \text{kv\_heads} \times \text{head\_dim} \times 2 \text{ (f16 bytes)}$$
   $$\text{Total KV Bytes} = \text{Bytes per Token} \times \text{max\_tokens}$$
4. **Config Override**: If declared in `SchedulerConfig::model_overrides`, custom allocations take precedence.

---

## Admission Control & LRU Eviction

When an agent requests GPU execution:

```text
Agent requests model "qwen2.5:0.5b"
         │
         ▼
┌──────────────────────────────────────────────┐
│ 1. Acquire execution permit (out of 32)      │
└──────────────────────┬───────────────────────┘
                       ▼
┌──────────────────────────────────────────────┐
│ 2. Compute exact required VRAM:              │
│    Weights (if not loaded) + KV-Cache Size   │
└──────────────────────┬───────────────────────┘
                       ▼
┌──────────────────────────────────────────────┐
│ 3. Admission Loop:                           │
│    Is (Free VRAM - Reserved) >= Required +   │
│       Safety Margin (512MB)?                 │
│                                              │
│    YES ──────────────▶ ADMIT REQUEST         │
│     │                                        │
│     NO                                       │
│     ▼                                        │
│    Find oldest idle model (active_requests=0)│
│    Found?                                    │
│      YES ────────────▶ Drop lock & unload    │
│                        model via driver      │
│                        (Loop repeats)        │
│      NO  ────────────▶ Return VRAM Exhausted │
│                        Error                 │
└──────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────┐
│ 4. Update Registry, reserve memory           │
│    and return RAII GpuLease                  │
└──────────────────────────────────────────────┘
```

### Safe Lock Dropping during Eviction

Unloading a heavy model from VRAM involves disk I/O and GPU driver calls that can take hundreds of milliseconds. To avoid freezing the entire kernel, the scheduler:
1. Identifies the candidate model under lock.
2. **Releases the state lock**.
3. Calls `driver.unload_model(&id).await` asynchronously.
4. Re-acquires the lock to update `ModelRegistry`.

---

## Design Decisions

| Decision | Rationale |
|---|---|
| **Multi-Tenancy vs. Semaphore(1)** | Enables small helper models (e.g., embedders, fast reasoning models) to stay resident alongside main models without thrashing VRAM. |
| **Physical NVML Queries** | Operating systems lie about allocated memory. Direct NVML queries read true physical hardware memory state. |
| **Pre-Calculated KV Footprint** | Prevents middle-of-generation OOM crashes by guaranteeing KV cache headroom *before* inference starts. |
| **Safety Margin (512 MB)** | Accommodates GPU driver allocations, display servers, and CUDA context overhead. |
| **Asynchronous RAII Drop** | Releasing permits and KV reservations happens in a detached task upon lease drop, preventing latency on the request critical path. |

---

**← Back to:** [Kernel Internals Index](./README.md)
