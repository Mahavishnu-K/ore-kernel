# Native Candle Engine

> Pure-Rust GGUF inference — zero external dependencies.

**Source:** [`ore-core/src/native/`](../../ore-core/src/native/)

---

## Overview

The Native Candle Engine is ORE's built-in inference backend, powered by Hugging Face's [Candle](https://github.com/huggingface/candle) framework. It runs quantized GGUF models directly on CPU, CUDA, or Metal — no Python, no daemon, no external runtime.

```
ore-core/src/native/
├── mod.rs               NativeDriver (entry point, model loading, hardware detection)
├── engine.rs            OreEngine enum (dispatches to architecture-specific loaders)
├── gguf_tokenizer.rs    GGUF metadata tokenizer extractor with JIT caching
└── models/
    ├── llama.rs          Llama family model loader
    ├── qwen.rs           Qwen2 family model loader
    └── bert.rs           BERT embedder (Safetensors, mean pooling, L2 norm)
```

---

## Hardware Detection

On boot, the `NativeDriver` probes for available compute hardware:

```
CUDA available?  → Use CUDA
Metal available? → Use Metal
Neither?         → Fall back to CPU
```

The selected `candle_core::Device` is passed to all model loaders. No manual configuration required.

---

## Model Loading (GGUF)

When an inference request arrives and the model isn't loaded:

1. **Locate the model** — Searches `models/<model_name>/` for `.gguf` files
2. **Read GGUF metadata** — Extracts architecture type from `general.architecture` field
3. **Route to loader** — Dispatches to Llama or Qwen2 loader based on architecture
4. **Store in engine** — The loaded model is held in memory as an `ActiveEngine` until evicted

### `OreEngine` Enum

```rust
pub enum OreEngine {
    Llama(/* architecture-specific model state */),
    Qwen(/* architecture-specific model state */),
}
```

Each variant wraps the architecture-specific model loader and implements a common interface for text generation.

---

## Tokenizer Resolution (3-Tier)

Finding the right tokenizer is surprisingly hard. The engine uses a cascading resolution strategy:

```
Tier 1: Model-specific tokenizer
        models/<model>/tokenizer.json
        ↓ (not found?)

Tier 2: Global tokenizer directory
        tokenizers/<family>.json
        ↓ (not found?)

Tier 3: Extract from GGUF metadata
        Read tokenizer data from .gguf file
        JIT-cache to disk for future loads
```

### Tier 3: GGUF Metadata Extraction

Source: [`ore-core/src/native/gguf_tokenizer.rs`](../../ore-core/src/native/gguf_tokenizer.rs)

Some GGUF files embed tokenizer data in their metadata. The engine extracts this data, constructs a tokenizer JSON, and **caches it to disk** so subsequent loads skip the extraction. This is a JIT (just-in-time) caching strategy — the first load is slow, every load after is instant.

---

## Streaming Token Generation

Tokens are generated one-at-a-time and sent through a `tokio::sync::mpsc::UnboundedSender<String>`:

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
Streamed to client as text/event-stream
```

This enables real-time streaming — the client sees tokens as they're generated, not all at once after completion.

---

## Native BERT Embedder

**Source:** [`ore-core/src/native/models/bert.rs`](../../ore-core/src/native/models/bert.rs)

A built-in `SystemEmbedder` that generates embedding vectors using BERT-architecture models loaded from Safetensors.

### Architecture

```
Input Text → Tokenize → BERT Forward Pass → Hidden States
     → Masked Mean Pooling → L2 Normalization → Embedding Vector
```

### Key Details

- **Model format:** Safetensors (full-precision weights)
- **Default model:** `all-MiniLM-L6-v2` (pulled via `ore pull system-embedder`)
- **Pooling:** Masked mean pooling — averages hidden states across non-padding tokens
- **Normalization:** L2 normalization to unit vectors for cosine similarity
- **Memory:** Zero-RAM idle design — when the embedding thread completes, Rust's ownership model automatically drops the model and frees all allocated memory

### Zero-RAM Idle Design

The BERT model is loaded, used, and dropped within a single function scope. Rust's ownership semantics guarantee that when the embedding computation finishes:

1. The model weights are dropped
2. All intermediate tensors are freed
3. RAM returns to 0MB idle

No manual memory management. No garbage collector. The type system enforces it.

---

## Supported Model Formats

| Format | Type | Models | Pull Command |
|---|---|---|---|
| **GGUF** | Quantized weights | Llama 3.2, Qwen 2.5, etc. | `ore pull llama3.2:1b` |
| **Safetensors** | Full-precision weights | BERT embedders | `ore pull system-embedder` |

### GGUF vs Safetensors

| | GGUF | Safetensors |
|---|---|---|
| **Purpose** | Text generation (LLMs) | Embeddings (BERT) |
| **Precision** | Quantized (Q4, Q8, etc.) | Full precision (f32) |
| **Size** | Smaller (quantized) | Larger (full weights) |
| **Metadata** | Includes architecture info, can embed tokenizer | Requires separate config files |

---

## Adding New Architectures

To add support for a new model architecture (Mistral, Phi, Gemma, etc.), see [Extending ORE → Adding a New Model Architecture](../extending-ore.md#2-adding-a-new-model-architecture-native-engine).

---

**← Back to:** [Kernel Internals Index](./README.md)
