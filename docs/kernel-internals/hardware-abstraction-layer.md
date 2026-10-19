# Hardware Abstraction Layer

> The trait that makes Ore engine-agnostic.

**Source:** [`ore-core/src/driver.rs`](../../ore-core/src/driver.rs)

---

## Overview

The Hardware Abstraction Layer (HAL) is a single Rust trait - `InferenceDriver` - that decouples all kernel logic from the physical inference engine. Applications and subsystems interact with the driver through this trait, never through engine-specific APIs directly.

Two implementations ship today:

| Driver | Engine | Source |
|---|---|---|
| `OllamaDriver` | Ollama daemon (HTTP proxy) | `ore-core/src/inference/external/ollama.rs` |
| `NativeDriver` | Pure-Rust Candle (GGUF) | `ore-core/src/inference/native/mod.rs` |

---

## The `InferenceDriver` Trait

```rust
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    /// Human-readable engine name (e.g., "Native Candle Engine")
    fn engine_name(&self) -> &'static str;

    /// Compute device in use, shown in telemetry (default: "Default Device")
    fn device_name(&self) -> String {
        "Default Device".to_string()
    }

    /// Health check - is the backend reachable?
    async fn is_online(&self) -> bool;

    /// List models currently loaded in VRAM
    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError>;

    /// Evict a model from VRAM
    async fn unload_model(&self, model: &str) -> Result<(), DriverError>;

    /// Pre-load a model into VRAM for zero-latency inference
    async fn preload_model(&self, model: &str) -> Result<(), DriverError>;

    /// Download and install a model
    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError>;

    /// List all models installed on disk
    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError>;

    /// Run inference - stream tokens through the mpsc channel.
    /// `app_id` + `stateful_paging` let the driver page this agent's KV cache in and out;
    /// `current_fingerprint` identifies the conversation prefix the saved cache must match.
    async fn generate_text(
        &self,
        model: &str,
        app_id: &str,
        stateful_paging: bool,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
        current_fingerprint: &str,
    ) -> Result<(), DriverError>;

    /// Generate embedding vectors for a batch of texts
    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError>;

    /// Drop the engine from RAM if it has been idle longer than the timeout
    async fn flush_idle_memory(&self, idle_timeout_mins: u64) -> Result<(), DriverError>;

    /// Wipe a specific agent's KV-Cache from RAM (Memory Compaction)
    async fn invalidate_agent_cache(&self, app_id: &str) -> Result<(), DriverError>;

    /// Exact token count using the engine's own tokenizer, or `None` if the driver
    /// has no direct tokenizer access (default: `None`)
    async fn count_tokens(&self, _model: &str, _text: &str) -> Option<u32> {
        None
    }
}
```

The trait has **13 methods**: 11 are required, and `device_name` and `count_tokens` have default implementations.

### Trait Bounds: `Send + Sync`

The driver is stored as `Arc<dyn InferenceDriver>` in `KernelState` and shared across all async tasks. The `Send + Sync` bounds ensure this is safe.

---

## Shared Types

### `VramProcess`

```rust
pub struct VramProcess {
    pub model_name: String,
    pub size_bytes: u64,
    pub size_vram_bytes: u64,
    pub status: String,          // e.g. Loaded / Loading (defaults to "")
    pub active_requests: usize,  // in-flight requests on this model (defaults to 0)
    pub last_used_secs: u64,     // seconds since last use (defaults to 0)
}
```

Represents a model currently occupying GPU VRAM. Used by `ore ps` and the `/ps` route. Defined in `ore-common/src/types.rs` and re-exported from `driver.rs`.

### `LocalModel`

```rust
pub struct LocalModel {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub format: String,          // e.g. GGUF / Safetensors (defaults to "")
}
```

Represents a model installed on disk. Used by `ore ls` and the `/ls` route.

### `DriverError`

```rust
#[derive(Error, Debug)]
pub enum DriverError {
    #[error("Driver Offline or Unreachable: {0}")]
    ConnectionFailed(String),

    #[error("API Error: {0}")]
    ApiError(String),

    #[error("Execution Failed: {0}")]
    ExecutionFailed(String),
}
```

---

## Token Counting

Context-window checks in `ore-server/src/handlers/inference.rs` use two tiers:

1. **Fast estimate:** `ore_common::estimate_tokens_calibrated` approximates the history size without a tokenizer.
2. **Exact count:** only when the estimate reaches **80%** of the agent's token limit does the kernel call `driver.count_tokens(model, text)`. `NativeDriver` answers with the loaded model's tokenizer (or the one on disk); drivers that return `None`, such as `OllamaDriver`, fall back to the estimate.

This keeps the common path cheap while making the decision to compact or summarize history exact near the limit.

---

## How Engine Swapping Works

The kernel decides which driver to instantiate at boot based on `ore.toml`:

```rust
// ore-server/src/main.rs
let driver: Arc<dyn InferenceDriver> = if config.system.engine == "native" {
    Arc::new(NativeDriver::new())
} else {
    Arc::new(OllamaDriver::default()) // constants::DEFAULT_OLLAMA_URL
};
```

After this point, **all kernel code is engine-agnostic**. The scheduler, firewall, handlers, and IPC layer only interact with `&dyn InferenceDriver` - they don't know or care which backend is running.

To switch engines:

```toml
# ore.toml
[system]
engine = "native"   # ← change to "ollama"
```

Then reboot the kernel. Zero code changes required.

---

## Driver Implementations

### `OllamaDriver`

**Source:** [`ore-core/src/inference/external/ollama.rs`](../../ore-core/src/inference/external/ollama.rs)

HTTP proxy to a running Ollama daemon. All operations translate to Ollama API calls:

| Trait Method | Ollama API |
|---|---|
| `is_online` | `GET /` (health check) |
| `get_running_models` | `GET /api/ps` |
| `generate_text` | `POST /api/chat` (streaming) |
| `generate_embeddings` | `POST /api/embed` |
| `pull_model` | `POST /api/pull` |
| `unload_model` | `POST /api/generate` (`keep_alive: 0`) |
| `preload_model` | `POST /api/generate` (`keep_alive: -1`) |
| `list_local_models` | `GET /api/tags` |

### `NativeDriver`

**Source:** [`ore-core/src/inference/native/mod.rs`](../../ore-core/src/inference/native/mod.rs)

Pure-Rust inference using Candle. See [Native Candle Engine](./native-candle-engine.md) for the full deep-dive.

---

## Adding a New Driver

See [Extending Ore → Adding a New Inference Driver](../extending-ore.md#1-adding-a-new-inference-driver) for a step-by-step guide.

Key requirements:
1. Implement the 11 required trait methods (override `device_name` and `count_tokens` if your backend can answer them)
2. Stream tokens through the `UnboundedSender<String>` channel
3. Use `DriverError` for all error reporting
4. Register the module and wire it into the boot sequence

---

**← Back to:** [Kernel Internals Index](./README.md)
