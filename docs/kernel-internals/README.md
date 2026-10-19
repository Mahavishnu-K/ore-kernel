# Kernel Internals

> Deep-dives into each Ore subsystem. Read these before contributing to core.

| Subsystem | Source File | Description |
|---|---|---|
| [Context Firewall](./context-firewall.md) | `ore-core/src/firewall.rs` | 2-stage prompt filter: injection heuristics + DLP redaction |
| [GPU Scheduler](./gpu-scheduler.md) | `ore-core/src/scheduler.rs` | Multi-tenant VRAM bin-packing, dynamic KV cache estimation & LRU eviction |
| [Memory Management](./memory-management.md) | `ore-core/src/memory.rs` | JSON history, fingerprinted KV-cache paging & compaction |
| [IPC & Semantic Bus](./ipc-and-semantic-bus.md) | `ore-core/src/ipc.rs` | Bounded agent messaging + vector memory with SSD paging |
| [Hardware Abstraction Layer](./hardware-abstraction-layer.md) | `ore-core/src/driver.rs` | Trait-based driver system for inference backends |
| [Native Candle Engine](./native-candle-engine.md) | `ore-core/src/inference/native/` | Pure-Rust GGUF inference (Llama, Qwen2/3, Qwen3-MoE) + BERT/Nomic embedders |
| [Polyglot Memory Fusion](./polyglot-memory-fusion.md) | `ore-core/src/linker/` | `dlopen`-style dynamic linker (`ore-ld`): load, bind, unload, errors, zero-copy calls |
| [Zero-Trust WASM Sandbox](./wasm-sandbox.md) | `ore-core/src/sandbox/` | Multi-mode execution (Tool/Script/Shell), AOT caching, network portal, VFS & result reporting |

## How to Read These Docs

Each document follows the same structure:

1. **What it does** - One-paragraph overview
2. **Data Structures** - The key structs and enums
3. **How it works** - Step-by-step code walkthrough
4. **Design decisions** - Why it was built this way
5. **Extension points** - How to add to it

---

**← Back to:** [Documentation Index](../README.md)
