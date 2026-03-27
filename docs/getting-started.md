# Getting Started

> From zero to local inference in under 5 minutes.

## Prerequisites

| Requirement | Details |
|---|---|
| **Rust toolchain** | `cargo` 1.93+ ([rustup.rs](https://rustup.rs/)) |
| **For Native engine** | No additional dependencies |
| **For Ollama engine** | [Ollama](https://ollama.ai/) running as the hardware driver |

## 1. Clone & Build

```bash
git clone https://github.com/Mahavishnu-K/ore-kernel.git
cd ore-kernel

# Install the ORE CLI globally
cargo install --path ore-cli
```

## 2. Initialize the System

Run the interactive setup wizard to choose your inference engine and configure memory management:

```bash
ore init
```

```
==================================================
 ORE KERNEL :: SYSTEM INITIALIZATION
==================================================
> Select your primary AI Execution Engine
  Ollama (Background daemon, easiest setup)
  Native (Bare-metal Rust execution, maximum control)

>>> CONFIGURING: RAM GARBAGE COLLECTION (GC)
    (How long should the OS keep idle Agent data in RAM?)
Mathematical Cache TTL in hours [0 = Infinite]: 24
Semantic Pipe TTL in hours [0 = Infinite]: 32
```

This generates your `ore.toml` configuration file. See [Configuration Reference](./configuration.md) for all options.

## 3. Boot the Kernel Daemon

```bash
# Terminal 1 — start the daemon
cargo run -p ore-server
```

Expected output:

```
=== ORE SYSTEM KERNEL BOOTING ===
-> [SECURITY] Master Token generated and secured to disk.
-> Sweeping /manifests for installed Apps...
-> [REGISTRY] Verified & Loaded App: openclaw
-> [REGISTRY] Verified & Loaded App: terminal_user
-> [REGISTRY] Verified & Loaded App: writer_agent
-> [BOOT] Engaging Native Candle Engine...
=== ORE KERNEL IS ONLINE ===
Listening on http://127.0.0.1:3000
```

> **⚡ Performance Tip:** Use `cargo run --release -p ore-server` for production workloads. The release profile enables `opt-level = 3`, LTO, and single codegen unit — making Native Candle inference **5–10x faster** than debug builds.

## 4. Download Models (Native Engine)

```bash
# Pull a GGUF model (streams from HuggingFace)
ore pull qwen2.5:0.5b
ore pull llama3.2:1b

# Pull the system embedder for the Semantic Bus
ore pull system-embedder
```

For Ollama engine users, pull models via `ollama pull <model>` instead.

## 5. Your First Inference

```bash
# Streamed inference through the firewall + scheduler
ore run qwen2.5:0.5b "Explain what a semaphore is in operating systems"
```

## 6. Register Your First Agent

Every application that talks to ORE needs a manifest. Generate one interactively:

```bash
ore manifest my_agent
```

The wizard walks you through selecting subsystem permissions: privacy, resources, file system, network, execution, and IPC. The generated `.toml` file is saved to `manifests/my_agent.toml`.

See [Manifest Reference](./manifest-reference.md) for the full schema.

## 7. Verify Everything Works

```bash
ore status          # Kernel online?
ore top             # View kernel telemetry
ore ps              # Models loaded in VRAM?
ore ls              # Models installed on disk?
ore ls --agents     # All registered agents + security status
```

## What's Next?

- **[Architecture](./architecture.md)** — Understand the full request lifecycle and how the subsystems connect
- **[CLI Reference](./cli-reference.md)** — Every command with examples
- **[API Reference](./api-reference.md)** — Build your own client against the HTTP API
- **[Security Model](./security-model.md)** — How ORE protects you from your own agents
- **[Kernel Internals](./kernel-internals/)** — Deep-dive into each subsystem

---

**Next:** [Architecture →](./architecture.md)
