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
| **Build dynamic WASM plugins** | [Building Plugins](./building-plugins.md) |
| **Add a new driver or model** | [Extending ORE](./extending-ore.md) |

## Kernel Internals

Deep-dives into each subsystem for contributors who want to understand ORE's brain before touching the code.

| Subsystem | Source | Doc |
|---|---|---|
| Context Firewall | `ore-core/src/firewall.rs` | [Deep Dive](./kernel-internals/context-firewall.md) |
| GPU Scheduler | `ore-core/src/scheduler.rs` | [Deep Dive](./kernel-internals/gpu-scheduler.md) |
| Memory Management | `ore-core/src/memory.rs` | [Deep Dive](./kernel-internals/memory-management.md) |
| IPC & Semantic Bus | `ore-core/src/ipc.rs` | [Deep Dive](./kernel-internals/ipc-and-semantic-bus.md) |
| Hardware Abstraction Layer | `ore-core/src/driver.rs` | [Deep Dive](./kernel-internals/hardware-abstraction-layer.md) |
| Native Candle Engine | `ore-core/src/native/` | [Deep Dive](./kernel-internals/native-candle-engine.md) |
| Polyglot Memory Fusion | `ore-core/src/linker/` | [Deep Dive](./kernel-internals/polyglot-memory-fusion.md) |

## Crate Map

```text
ore-system/
├── ore-core/       Kernel logic (sandbox, MMU, firewall, wasmtime runtime)
│   └── linker/     ore-ld: POSIX Dynamic Linker & Memory Fusion Engine
├── ore-server/     Axum HTTP daemon (routes, auth middleware, state)
├── ore-cli/        Interactive CLI tool (ore init, ore mktool)
├── ore-sys/        Rust SDK Macros (`ore_bind!`, `ore_export!`, `Plugin`)
├── tests/          Polyglot Memory Fusion & Security boundary test suites
├── plugins/        Pre-compiled `.wasi.so` dynamic plugins
└── tools/          Pre-compiled `.wasm` Host Agents & Nano-Services
```

## Contributing

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for the code of conduct and PR process. Join us on [Discord](https://discord.com/channels/1477053099494342755/1477053558879686737) - we hang out in `#dev-core`.
