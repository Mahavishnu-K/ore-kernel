# Configuration Reference

> Every knob you can turn in the ORE kernel.

## `ore.toml` - System Configuration

The kernel reads `ore.toml` from the workspace root on boot. Generate it via `ore init` or edit manually.

### Full Schema

```toml
[system]
engine = "native"           # "native" or "ollama"
embedder = "all-minilm"     # "all-minilm" (90MB Fast) or "system-embedder" (500MB Nomic)

[native]
default_model = "llama3.2:1b"  # Default model for the Native Candle engine

[memory]
cache_ttl_hours = 24        # Embedding cache lifetime in hours (0 = infinite)
pipe_ttl_hours = 32         # Semantic pipe data lifetime in hours (0 = infinite)
```

### `[system]` Section

| Key | Type | Values | Description |
|---|---|---|---|
| `engine` | string | `"native"`, `"ollama"` | Which inference backend to use on boot |
| `embedder` | string | `"all-minilm"`, `"system-embedder"` | Which Native System Embedder architecture to use for Semantic Bus searching |

### `[native]` Section

Only read when `engine = "native"`.

| Key | Type | Default | Description |
|---|---|---|---|
| `default_model` | string | - | The model loaded by default when no model is specified |

### `[memory]` Section

Controls the kernel's garbage collector, which runs every hour.

| Key | Type | Default | Description |
|---|---|---|---|
| `cache_ttl_hours` | u64 | `24` | How long computed embeddings stay cached in RAM. Set to `0` for infinite retention |
| `pipe_ttl_hours` | u64 | `32` | How long semantic pipe data (memory chunks) survive before eviction. Set to `0` for infinite retention |

**How GC works:** A background `tokio::spawn` task wakes every 3600 seconds and calls `SemanticBus::run_garbage_collection()`. It walks the embedding cache and all memory pipes, evicting entries older than their configured TTL. Empty pipes are pruned automatically.

Source: [`ore-server/src/main.rs:89-96`](../ore-server/src/main.rs) · [`ore-core/src/ipc.rs:186-236`](../ore-core/src/ipc.rs)

---

## `rust-toolchain.toml`

Pins the Rust version for reproducible builds:

```toml
[toolchain]
channel = "1.93.0"
```

---

## `Cargo.toml` - Workspace & Release Profile

```toml
[workspace]
resolver = "2"
members = ["ore-common", "ore-core", "ore-server", "ore-cli"]

[profile.release]
opt-level = 3        # Maximum optimization
lto = true           # Link-time optimization (slower build, faster binary)
codegen-units = 1    # Single codegen unit (better optimization, slower compile)
strip = true         # Strip debug symbols
```

> **Why this matters:** The Native Candle engine runs inference entirely in Rust. Without release optimizations, token generation can be 5–10x slower. Always use `cargo run --release` for production workloads.

---

## Engine Comparison

| | Native (Candle) | Ollama |
|---|---|---|
| **Dependencies** | Zero - pure Rust | Requires Ollama daemon |
| **Model Format** | GGUF (quantized) | All Ollama-compatible |
| **Hardware** | CPU / CUDA / Metal (auto-detected) | Depends on Ollama |
| **Embeddings** | Built-in BERT (Safetensors) | Via `/api/embed` endpoint |
| **Best For** | Airgapped, embedded, maximum control | Easy setup, broad model support |

Switch engines by changing one line in `ore.toml` and rebooting.

---

**Next:** [CLI Reference →](./cli-reference.md)
