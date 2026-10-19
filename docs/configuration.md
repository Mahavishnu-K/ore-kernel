# Configuration Reference

> Every knob you can turn in the Ore kernel.

## `ore.toml` - System Configuration

The kernel reads `ore.toml` from the Ore base directory on boot. The base directory is resolved in the following order:
1. The `$ORE_DIR` environment variable, if set.
2. The local `../` directory, if `../ore.toml` exists (for local development).
3. The user's home directory (`~/.ore` on Linux/macOS, or `%USERPROFILE%\.ore` on Windows).

Generate it via `ore init` or edit manually.

### Full Schema

```toml
[system]
engine = "native"           # "native" or "ollama"
embedder = "all-minilm"     # "all-minilm" (90MB Fast) or "system-embedder" (500MB Nomic)

[native]
default_model = "llama3.2:1b"  # Written by `ore init`; not read by the kernel yet

[memory]
cache_ttl_hours = 24        # Embedding cache lifetime in hours (0 = infinite)
pipe_ttl_hours = 32         # Semantic pipe data lifetime in hours (0 = infinite)

# Optional, per model: override the scheduler's KV-cache estimate
[models."qwen2.5:0.5b"]
kv_cache_mb_per_1k = 12     # MB of KV cache per 1,000 tokens of context
```

### `[system]` Section

| Key | Type | Values | Description |
|---|---|---|---|
| `engine` | string | `"native"`, `"ollama"` | Which inference backend to use on boot |
| `embedder` | string | `"all-minilm"` (default), `"system-embedder"` | Which embedding model the Semantic Bus uses. Optional |

`[system]` and `[memory]` are both **required**: the kernel won't boot if either is missing.

### `[native]` Section

Written by `ore init` when you choose the native engine.

| Key | Type | Default | Description |
|---|---|---|---|
| `default_model` | string | - | Recorded for tooling. The kernel doesn't read it yet: `/ask` uses the first concrete model in the agent's `allowed_models`, and `/run` uses the model in the request |

### `[memory]` Section

Controls the Semantic Bus garbage collector, which runs every hour.

| Key | Type | Default | Description |
|---|---|---|---|
| `cache_ttl_hours` | u64 | `24` | How long computed embeddings stay cached in RAM. Set to `0` for infinite retention |
| `pipe_ttl_hours` | u64 | `32` | How long semantic pipe data (memory chunks) survive before eviction. Set to `0` for infinite retention |

**How the background loop works:** a `tokio::spawn` task in `ore-server/src/main.rs` wakes **every minute**:

- Every tick, it calls `driver.flush_idle_memory(5)`, which unloads an engine that's been idle for more than 5 minutes.
- Every 60th tick (hourly), it calls `SemanticBus::run_garbage_collection()`, which walks the embedding cache and all memory pipes and evicts entries older than their TTL. Empty pipes are pruned.

Source: [`ore-server/src/main.rs`](../ore-server/src/main.rs) · [`ore-core/src/ipc.rs`](../ore-core/src/ipc.rs)

### `[models."<name>"]` Sections (optional)

Per-model overrides for the GPU scheduler, keyed by model name.

| Key | Type | Default | Description |
|---|---|---|---|
| `kv_cache_mb_per_1k` | u64 | *(estimated from GGUF geometry)* | MB of KV cache per 1,000 tokens of context. The scheduler reserves `ceil(value × 8192 / 1000)` MB per request on this model (e.g. `12` → 99 MB), replacing the automatic estimate of `2 × layers × kv_heads × head_dim × 2 bytes × 8192 tokens`. Use it when the automatic estimate is wrong for a model |

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `ORE_DIR` | *(see resolution order above)* | Ore base directory: `ore.toml`, `manifests/`, `models/`, `memory/`, `runtimes/`, `tools/`, `plugins/` |
| `ORE_GPU_ID` | `0` | NVIDIA GPU index the scheduler reads VRAM from through NVML. If NVML isn't available, the scheduler falls back to system-memory figures |
| `HF_TOKEN` | - | Hugging Face token for `ore pull` of gated models |

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
members = ["ore-common", "ore-core", "ore-server", "ore-cli", "ore-sys"]

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
| **Architectures** | Llama, Qwen2, Qwen3, Qwen3-MoE | Anything Ollama supports |
| **KV-cache paging** | Yes (`stateful_paging`) | No (JSON history only) |
| **Exact token counts** | Yes (`count_tokens`) | No (estimates) |
| **Embeddings** | Built-in BERT / Nomic (Safetensors) | Via `/api/embed` endpoint |
| **Best For** | Airgapped, embedded, maximum control | Easy setup, broad model support |

Switch engines by changing one line in `ore.toml` and rebooting.

---

**Next:** [CLI Reference →](./cli-reference.md)
