# ORE Documentation

> **The Manual for the Kernel.**
> This folder contains everything you need to understand how ORE works internally, how to configure it, how to extend it, and how to build on top of it.

## Where to Start

| You want to... | Read this |
|---|---|
| **Get ORE running** | [Getting Started](./getting-started.md) |
| **Understand the big picture** | [Architecture](./architecture.md) |
| **Configure the kernel** | [Configuration Reference](./configuration.md) |
| **Use the CLI** | [CLI Reference](./cli-reference.md) |
| **Hit the HTTP API** | [API Reference](./api-reference.md) |
| **Write a secure manifest** | [Manifest Reference](./manifest-reference.md) |
| **Understand the security model** | [Security Model](./security-model.md) |
| **Add a new driver or model** | [Extending ORE](./extending-ore.md) |

## Kernel Internals

Deep-dives into each subsystem for contributors who want to understand ORE's brain before touching the code.

| Subsystem | Source | Doc |
|---|---|---|
| Context Firewall | `ore-core/src/firewall.rs` | [Deep Dive](./kernel-internals/context-firewall.md) |
| GPU Scheduler | `ore-core/src/scheduler.rs` | [Deep Dive](./kernel-internals/gpu-scheduler.md) |
| SSD Pager | `ore-core/src/swap.rs` | [Deep Dive](./kernel-internals/ssd-pager.md) |
| IPC & Semantic Bus | `ore-core/src/ipc.rs` | [Deep Dive](./kernel-internals/ipc-and-semantic-bus.md) |
| Hardware Abstraction Layer | `ore-core/src/driver.rs` | [Deep Dive](./kernel-internals/hardware-abstraction-layer.md) |
| Native Candle Engine | `ore-core/src/native/` | [Deep Dive](./kernel-internals/native-candle-engine.md) |

## Crate Map

```
ore-system/
├── ore-common/     Shared types (InferenceRequest, InferenceResponse, ModelId)
├── ore-core/       Kernel logic (firewall, scheduler, IPC, drivers, native engine)
├── ore-server/     Axum HTTP daemon (routes, auth middleware, state)
└── ore-cli/        Interactive CLI tool (clap + dialoguer + HuggingFace Hub)
```

## Contributing

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for the code of conduct and PR process. Join us on [Discord](https://discord.com/channels/1477053099494342755/1477053558879686737) — we hang out in `#dev-core`.
