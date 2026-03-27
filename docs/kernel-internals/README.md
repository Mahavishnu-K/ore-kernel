# Kernel Internals

> Deep-dives into each ORE subsystem. Read these before contributing to core.

| Subsystem | Source File | Description |
|---|---|---|
| [Context Firewall](./context-firewall.md) | `ore-core/src/firewall.rs` | 3-stage prompt security pipeline |
| [GPU Scheduler](./gpu-scheduler.md) | `ore-core/src/scheduler.rs` | Semaphore-based scheduling with RAII leases |
| [SSD Pager](./ssd-pager.md) | `ore-core/src/swap.rs` | OS-style context persistence to disk |
| [IPC & Semantic Bus](./ipc-and-semantic-bus.md) | `ore-core/src/ipc.rs` | Agent messaging + vector memory database |
| [Hardware Abstraction Layer](./hardware-abstraction-layer.md) | `ore-core/src/driver.rs` | Trait-based driver system for inference backends |
| [Native Candle Engine](./native-candle-engine.md) | `ore-core/src/native/` | Pure-Rust GGUF inference + BERT embedder |

## How to Read These Docs

Each document follows the same structure:

1. **What it does** — One-paragraph overview
2. **Data Structures** — The key structs and enums
3. **How it works** — Step-by-step code walkthrough
4. **Design decisions** — Why it was built this way
5. **Extension points** — How to add to it

---

**← Back to:** [Documentation Index](../README.md)
