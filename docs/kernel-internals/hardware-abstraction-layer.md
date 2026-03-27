# Hardware Abstraction Layer

> The trait that makes ORE engine-agnostic.

**Source:** [`ore-core/src/driver.rs`](../../ore-core/src/driver.rs)

---

## Overview

The Hardware Abstraction Layer (HAL) is a single Rust trait — `InferenceDriver` — that decouples all kernel logic from the physical inference engine. Applications and subsystems interact with the driver through this trait, never through engine-specific APIs directly.

Two implementations ship today:

| Driver | Engine | Source |
|---|---|---|
| `OllamaDriver` | Ollama daemon (HTTP proxy) | `ore-core/src/external/ollama.rs` |
| `NativeDriver` | Pure-Rust Candle (GGUF) | `ore-core/src/native/mod.rs` |

---

## The `InferenceDriver` Trait

```rust
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    /// Human-readable engine name (e.g., "Native Candle Engine")
    fn engine_name(&self) -> &'static str;

    /// Health check — is the backend reachable?
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

    /// Run inference — stream tokens through the mpsc channel
    async fn generate_text(
        &self,
        model: &str,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
    ) -> Result<(), DriverError>;

    /// Generate embedding vectors for a batch of texts
    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError>;
}
```

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
}
```

Represents a model currently occupying GPU VRAM. Used by `ore ps` and the `/ps` route.

### `LocalModel`

```rust
pub struct LocalModel {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
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

## How Engine Swapping Works

The kernel decides which driver to instantiate at boot based on `ore.toml`:

```rust
// ore-server/src/main.rs
let driver: Arc<dyn InferenceDriver> = if config.system.engine == "native" {
    Arc::new(NativeDriver::new())
} else {
    Arc::new(OllamaDriver::new("http://127.0.0.1:11434"))
};
```

After this point, **all kernel code is engine-agnostic**. The scheduler, firewall, handlers, and IPC layer only interact with `&dyn InferenceDriver` — they don't know or care which backend is running.

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

**Source:** [`ore-core/src/external/ollama.rs`](../../ore-core/src/external/ollama.rs)

HTTP proxy to a running Ollama daemon. All operations translate to Ollama API calls:

| Trait Method | Ollama API |
|---|---|
| `is_online` | `GET /` (health check) |
| `get_running_models` | `GET /api/ps` |
| `generate_text` | `POST /api/chat` (streaming) |
| `generate_embeddings` | `POST /api/embed` |
| `pull_model` | `POST /api/pull` |
| `unload_model` | `POST /api/generate` (keep_alive: 0) |
| `list_local_models` | `GET /api/tags` |

### `NativeDriver`

**Source:** [`ore-core/src/native/mod.rs`](../../ore-core/src/native/mod.rs)

Pure-Rust inference using Candle. See [Native Candle Engine](./native-candle-engine.md) for the full deep-dive.

---

## Adding a New Driver

See [Extending ORE → Adding a New Inference Driver](../extending-ore.md#1-adding-a-new-inference-driver) for a step-by-step guide.

Key requirements:
1. Implement all 9 trait methods
2. Stream tokens through the `UnboundedSender<String>` channel
3. Use `DriverError` for all error reporting
4. Register the module and wire it into the boot sequence

---

**← Back to:** [Kernel Internals Index](./README.md)
