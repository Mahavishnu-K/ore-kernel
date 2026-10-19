# Native Candle Engine

> Pure-Rust GGUF inference - zero external dependencies.

**Source:** [`ore-core/src/inference/native/`](../../ore-core/src/inference/native/)

---

## Overview

The Native Candle Engine is Ore's built-in inference backend, powered by Hugging Face's [Candle](https://github.com/huggingface/candle) framework. It runs quantized GGUF models directly on CPU, CUDA, or Metal - no Python, no daemon, no external runtime.

```
ore-core/src/inference/native/
├── mod.rs               NativeDriver (hardware detection, agent switching, generation, token counting)
├── engine.rs            OreEngine enum + architecture router (OreEngine::load)
├── kv_manager.rs        Flattens / restores KV-cache tensors for .safetensors paging
├── gguf_tokenizer.rs    GGUF metadata tokenizer extractor with JIT caching
└── models/
    ├── llama.rs          Llama family
    ├── qwen2.rs          Qwen2 family
    ├── qwen3.rs          Qwen3 (dense)
    ├── qwen3_moe.rs      Qwen3-MoE (also loads qwen2moe GGUFs)
    ├── transformers/     Quantized layers, quantized var builder, fused MoE
    ├── nn/               KV cache
    ├── utils/            Tracing helpers
    ├── bert.rs           BERT embedder (all-MiniLM)
    └── nomic.rs          Nomic v1.5 embedder (NomicBertModel with custom RoPE/SwiGLU)
```

---

## Hardware Detection

On boot, the `NativeDriver` probes for available compute hardware:

```
CUDA available?  → Use CUDA device 0
Metal available? → Use Metal device 0
Neither (or init fails)? → Fall back to CPU
```

The selected `candle_core::Device` is passed to all model loaders. No manual configuration required.

---

## Model Loading (GGUF)

When an inference request arrives and the model isn't loaded:

1. **Locate the model** - Searches `models/<model_name>/` for `.gguf` files (`:` in the name becomes `-`)
2. **Instant Boot (`memmap2`)** - Maps the file directly into virtual memory using the OS's `mmap` syscall, achieving sub-50ms boot times by letting the OS lazily stream required weight pages into the GPU instead of stalling system RAM.
3. **Read GGUF metadata** - Extracts architecture type from `general.architecture` field via the memory-mapped cursor without reading the entire file.
4. **Route to loader** - `OreEngine::load` matches `general.architecture`: `llama`, `qwen2`, `qwen3`, or `qwen3moe` / `qwen2moe`. Anything else fails with `Architecture not supported natively`.
5. **Store in engine** - The loaded model and its memory-mapped file lock are held as an `ActiveEngine` until evicted.

### `OreEngine` Enum

```rust
pub enum OreEngine {
    Qwen2(Qwen2Model),
    Llama(LlamaModel),
    Qwen3(Qwen3Model),
    Qwen3Moe(Qwen3MoeModel),
}
```

`impl OreEngine` gives every architecture the same interface: `forward`, `num_layers`, and the KV-cache operations used by paging (`clear_kv_cache`, `truncate_kv_cache`, `get_kv_cache_len`, `get_kv_cache`, `set_kv_cache`).

Sampling uses Candle's `LogitsProcessor` with temperature `0.7` and a fixed seed.

---

## One Active Model, Many Agents

`NativeDriver` holds **one** active engine at a time (model weights + KV cache + the agent currently using it). On each `generate_text` call it compares the request with what's loaded:

| Situation | What happens |
|---|---|
| Same model, same agent | **Hot hit**: keep the KV cache in RAM and skip the SSD |
| Same model, different agent | **Agent swap**: freeze the old agent's KV cache to SSD (if it uses `stateful_paging`), then clear the cache but keep the weights |
| Different model | **Model reload**: freeze the old agent's KV cache, drop the weights, and memory-map the new model |

Then, for a `stateful_paging` agent that wasn't a hot hit, the driver calls `Pager::page_in_kv_cache`. It restores the saved cache only if its **fingerprint** matches the agent's current history, so a stale cache is never applied to a changed conversation. With a valid cache, prefill is skipped for the saved tokens; otherwise the cache is rebuilt from the JSON history.

The SSD write runs as a background task, and generation waits for it to finish before starting. See [Memory Management](./memory-management.md) for the file format.

---

## Token Counting

`NativeDriver` implements `count_tokens`: it encodes text with the active model's tokenizer (or the one on disk) and returns the exact count. The inference handler only asks for it when its fast estimate reaches 80% of the agent's context limit. See [Hardware Abstraction Layer → Token Counting](./hardware-abstraction-layer.md#token-counting).

---

## Tokenizer Resolution (2-Tier)

Finding the right tokenizer is surprisingly hard. The engine uses a cascading resolution strategy:

```
Tier 1: Model-specific tokenizer
        models/<model>/tokenizer.json
        ↓ (not found?)

Tier 2: Extract from GGUF metadata
        Read tokenizer data from .gguf file
        JIT-cache to disk for future loads
```

### Tier 2: GGUF Metadata Extraction

Source: [`ore-core/src/inference/native/gguf_tokenizer.rs`](../../ore-core/src/inference/native/gguf_tokenizer.rs)

Some GGUF files embed tokenizer data in their metadata. The engine extracts this data, constructs a tokenizer JSON, and **caches it to disk** so subsequent loads skip the extraction. This is a JIT (just-in-time) caching strategy - the first load is slow, every load after is instant.

---

## Streaming Token Generation

Tokens are generated one at a time inside `spawn_blocking` and sent through a `tokio::sync::mpsc::UnboundedSender<String>`:

```
Model generates token
     │
     ▼
tx.send(token_text)
     │
     ▼
Handler receives via rx
     │
     ▼
Streamed to the client as a chunked text body
```

This enables real-time streaming - the client sees tokens as they're generated, not all at once after completion.

---

## Native System Embedders

**Source:** [`ore-core/src/inference/native/models/bert.rs`](../../ore-core/src/inference/native/models/bert.rs) | [`ore-core/src/inference/native/models/nomic.rs`](../../ore-core/src/inference/native/models/nomic.rs)

The kernel includes a built-in `SystemEmbedder` abstraction that generates vector embeddings using Safetensor models. Two architectures are supported:

1. **BERT (`all-MiniLM-L6-v2`)** - The lightweight, ultra-fast default. (Models up to 90MB)
2. **Nomic (`nomic-embed-text-v1.5`)** - High accuracy, custom RoPE/SwiGLU architecture. (Models up to 500MB+)

### Architecture

```
Input Text → Tokenize → Model Forward Pass → Hidden States
     → Masked Mean Pooling → L2 Normalization → Embedding Vector
```

### Key Details

- **Model format:** Safetensors (full-precision weights)
- **Pooling:** Masked mean pooling - averages hidden states across non-padding tokens.
- **Normalization:** Vectors are immediately L2 normalized to unit lengths so the Semantic Bus can use cheap arithmetic dot-products during search.
- **Memory:** Zero-RAM idle design. When the embedding thread completes, Rust's ownership model drops the model and frees all allocated memory.

### The `embedder_lock` (Concurrency Safety)

Because embedder models can require upwards of 500MB of RAM/VRAM to load, parallel Semantic Bus API calls (`POST /ipc/share` or `POST /ipc/search`) could accidentally load the model 10 times simultaneously, blowing past available hardware limits in an Out-Of-Memory (OOM) crash.

To prevent this, the server handler layer wraps the embedding generation in a strict `Arc<Mutex<()>>` `embedder_lock`.

```rust
let _embedder_guard = state.embedder_lock.lock().await;
// Load model -> Generate -> Drop model
```

This enforces serialization: only **one thread** is allowed to instantiate the embedder weights into memory at a time. Other concurrent agent requests wait safely in an async queue.

### Zero-RAM Idle Design

The embedder model is loaded, used, and dropped within a single function scope. When the computation finishes:

1. The `_embedder_guard` unlocks (allowing the next request to proceed).
2. The model weights are dropped.
3. All intermediate tensors are freed.
4. RAM instantly returns to 0MB idle.

No manual memory management. No garbage collector. The Rust type system enforces it.

---

## Supported Model Formats

| Format | Type | Models | Pull Command |
|---|---|---|---|
| **GGUF** | Quantized weights | Llama 3.x, Qwen 2.5, Qwen 3, Qwen 3 MoE, DeepSeek-R1 distills (Qwen/Llama based) | `ore pull llama3.2:1b` |
| **Safetensors** | Full-precision weights | BERT (all-MiniLM) and Nomic embedders | `ore pull all-minilm`, `ore pull system-embedder` |

### GGUF vs Safetensors

| | GGUF | Safetensors |
|---|---|---|
| **Purpose** | Text generation (LLMs) | Embeddings (BERT / Nomic) |
| **Precision** | Quantized (Q4, Q8, etc.) | Full precision (f32) |
| **Size** | Smaller (quantized) | Larger (full weights) |
| **Metadata** | Includes architecture info, can embed tokenizer | Requires separate config files |

---

## Adding New Architectures

To add support for a new model architecture (Mistral, Phi, Gemma, etc.), see [Extending Ore → Adding a New Model Architecture](../extending-ore.md#2-adding-a-new-model-architecture-native-engine).

---

**← Back to:** [Kernel Internals Index](./README.md)
