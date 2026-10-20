# Getting Started

> From zero to local inference, a registered agent, and a sandboxed tool.

## Prerequisites

| Requirement | Details |
|---|---|
| **Rust toolchain** | `cargo` 1.93+ ([rustup.rs](https://rustup.rs/)) |
| **For the Native engine** | Nothing else |
| **For the Ollama engine** | [Ollama](https://ollama.com/) running as the inference backend |
| **For C, C++, or Zig tools** | Nothing: `ore mktool` installs a verified Zig 0.17.0 automatically on first use (or uses the `zig` on your `PATH`) |

## 1. Clone & Build

```bash
git clone https://github.com/Mahavishnu-K/ore-kernel.git
cd ore-kernel

# Install the Ore CLI globally
cargo install --path ore-cli
```

## 2. Initialize the System

Run the interactive setup wizard:

```bash
ore init
```

It asks for:

- **Execution engine:** `Native` (pure-Rust Candle inference) or `Ollama` (proxy to an Ollama daemon).
- **Semantic Bus embedder:** `all-minilm` (~90 MB, fast, good for laptops) or `system-embedder` (Nomic v1.5, ~500 MB, more accurate).
- **Garbage collection:** how many hours embeddings stay cached and semantic-pipe data survives (`0` = forever).

This writes `ore.toml` in the Ore base directory. See the [Configuration Reference](./configuration.md) for every option.

## 3. Boot the Kernel Daemon

```bash
# Terminal 1 - start the daemon (release build: 5–10x faster native inference)
cargo run --release -p ore-server
```

Expected output (abridged):

```text
=== ORE SYSTEM KERNEL BOOTING ===
-> [SECURITY] Master Token generated and secured to disk.
-> Sweeping /manifests for installed Apps...
-> [REGISTRY] Verified & Loaded App: openclaw
-> [BOOT] Engaging Native Candle Engine...
=== ORE KERNEL IS ONLINE ===
Listening on http://127.0.0.1:6767
```

The kernel writes a session token to `ore-kernel.token`. The CLI reads it automatically; HTTP clients send it as `Authorization: Bearer <token>`.

## 4. Download Models (Native Engine)

```bash
# A GGUF chat model (streamed from Hugging Face)
ore pull qwen2.5:0.5b

# The embedder you chose in `ore init` (for the Semantic Bus)
ore pull all-minilm          # or: ore pull system-embedder

# Optional: runtimes for Python / JavaScript scripts in the sandbox
ore pull system-py
ore pull system-js
```

With the Ollama engine, pull models with `ollama pull <model>` instead.

## 5. Your First Inference

```bash
# Streamed inference through the firewall and scheduler
ore run qwen2.5:0.5b "Explain what a semaphore is in operating systems"

# Interactive chat session (renders <think> blocks from reasoning models)
ore pull deepseek-r1:7b
ore run deepseek-r1:7b
```

## 6. Register Your First Agent

Every application that talks to Ore needs a manifest. Generate one interactively:

```bash
ore manifest my_agent
```

The wizard asks which subsystems the agent needs (Privacy, Resources, File System, Network, Execution, IPC) and walks you through each one. It saves `manifests/my_agent.toml`.

For the next step, make sure the **Execution** section allows WASM and lists the tool:

```toml
[execution]
can_execute_wasm = true
allowed_tools = ["hello"]
```

Restart the kernel after adding or editing a manifest; manifests are loaded at boot. See the [Manifest Reference](./manifest-reference.md) for the full schema.

## 7. Compile and Run a Sandboxed Tool

Ore compiles tools written in Python, Rust, Go, C, C++, Zig, JavaScript, and TypeScript to WebAssembly and runs them in its sandbox.

Create `hello.py`:

```python
import sys
print(f"Hello from the Ore Sandbox! Arguments: {sys.argv[1:]}")
```

Compile it (works offline; writes `~/.ore/tools/hello.wasm`):

```bash
ore mktool hello.py
```

Run it as `my_agent`:

```bash
curl -X POST http://127.0.0.1:6767/execute \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{"app_id": "my_agent", "tool_name": "hello", "args": ["one", "two"]}'
```

The response is the tool's output. If the tool fails (a trap, a non-zero exit, or running out of CPU fuel), the response ends with a `KERNEL ERROR:` line. See [`POST /execute`](./api-reference.md#post-execute).

## 8. Verify Everything Works

```bash
ore status          # Kernel online?
ore top             # Kernel telemetry
ore ps              # Models loaded in memory?
ore ls              # Models installed on disk?
ore ls --agents     # Registered agents + security status
```

## What's Next?

- **[Why Ore?](./why-ore.md)**: The problems Ore solves and the design behind each subsystem
- **[Architecture](./architecture.md)**: The full request lifecycle and how the subsystems connect
- **[Building Plugins](./building-plugins.md)**: Zero-copy Memory Fusion plugins with `ore-ld`
- **[CLI Reference](./cli-reference.md)**: Every command
- **[API Reference](./api-reference.md)**: Build your own client against the HTTP API
- **[Security Model](./security-model.md)**: What Ore enforces, and its known limits
- **[Kernel Internals](./kernel-internals/)**: Deep dives into each subsystem

---

**Next:** [Architecture →](./architecture.md)
