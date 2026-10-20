# CLI Reference

> Every command the Ore CLI provides, with syntax, flags, and example output.

The CLI binary is `ore`. Install it with `cargo install --path ore-cli`.

Most commands talk to the running kernel (`ore-server`) using the token in `ore-kernel.token`, and render its JSON responses as tables and dashboards. `ore init`, `ore pull`, `ore manifest`, and `ore mktool` work without the kernel.

---

## System Commands

### `ore init`

Interactive setup wizard that generates `ore.toml`.

```bash
ore init
```

Configures:
- **Engine selection** - Ollama (daemon-based) or Native (bare-metal Rust)
- **Engine defaults** - Model paths, API URLs
- **Memory GC** - Embedding cache TTL and semantic pipe TTL

---

### `ore status`

Check if the kernel daemon is online.

```bash
ore status
```

Shows an **ORE KERNEL SYSTEM STATUS** panel: kernel state and version, inference engine, compute device, home directory, loaded models, registered agents, and uptime. If the kernel isn't reachable, it says so.

---

### `ore top`

View kernel telemetry.

```bash
ore top
```

Shows host hardware and OS (CPU, RAM, load), the accelerator and its VRAM budget (total, used, free, reserved, safety margin), and the status of each subsystem: inference engine, scheduler, context firewall, Semantic Bus cache, and WASM sandbox.

---

### `ore ps`

Show models currently loaded in memory: engine, device, status, active requests, RAM / VRAM use, time since last use, and the agent currently using each one.

```bash
ore ps
```

---

### `ore ls`

List all locally installed models on disk.

```bash
ore ls
```

Lists each model's name, format (e.g. `GGUF (Quantized)`, `Safetensors`), size, last update, and whether it's loaded.

**Flags:**

| Flag | Description |
|---|---|
| `--models` | List all downloaded LLM models (default behavior) |
| `--agents` | List all registered agents with security status |
| `--manifests` | View raw permission matrix for all manifests |

```bash
ore ls --agents
```

Lists each agent's ID, version, allowed models, priority, network access, file-system access, execution engine, PII enforcement, and status.

Status values:
- **UNSAFE**: `can_execute_shell = true`
- **AIR-GAPPED**: no shell access, network disabled
- **SECURED**: no shell access, network enabled

```bash
ore ls --manifests
```

Shows the permission matrix per manifest file: network, file I/O, execution mode, and PII scrubbing.

---

## Model Management

### `ore pull <model>`

Download and install a model. Supports GGUF and Safetensors formats.

```bash
# GGUF models (quantized weights + tokenizer)
ore pull qwen2.5:0.5b
ore pull deepseek-r1:7b

# Safetensors (full-precision, for embeddings)
ore pull system-embedder

# WASM Runtimes (for Autonomous Scripts)
ore pull system-py
ore pull system-py-data   # CPython with data-science libraries (used by mktool --env data)
ore pull system-js
```

All downloads stream directly to `models/` or `runtimes/` with zero RAM bloat. Supports HuggingFace token for gated models (requires `HF_TOKEN` environment variable).

---

### `ore load <model>`

Pre-load a model into VRAM for zero-latency inference.

```bash
ore load qwen2.5:0.5b
```

---

### `ore expel <model>`

Forcefully evict a model from GPU VRAM.

```bash
ore expel qwen2.5:0.5b
```

---

## Inference

### `ore run <model> [prompt]`

Execute a secured inference request. If `[prompt]` is provided, it streams the output and exits. If omitted, it launches an **Interactive TUI Chat Session**.

```bash
# Single execution (streamed output)
ore run qwen2.5:0.5b "Explain what a semaphore is"

# Interactive Chat Session
ore run deepseek-r1:7b
```

**Interactive TUI Features:**
- **`<think>` Tag Parsing:** Automatically intercepts reasoning blocks from models like DeepSeek-R1, rendering the internal monologue in dim italic text, and the final answer in bold blue.
- Use `/e` or `/exit` to disconnect.

The prompt passes through the full firewall pipeline (injection detection → PII redaction) before reaching the model.

---

## Agent Management

### `ore manifest <app_id>`

Interactive wizard to generate a secure `.toml` manifest.

```bash
ore manifest my_agent
```

```text
 ╭─ Secure Manifest Forge ────────────────────────────╮
 │            Target Agent: my_agent                  │
 ╰────────────────────────────────────────────────────╯
 
 ? Select required sub-systems for this agent:
   [ ] Privacy      (PII Redaction)
   [ ] Resources    (GPU Quotas, Models, Paging)
   [ ] File System  (File System Boundaries)
   [ ] Network      (Egress Control)
   [ ] Execution    (WASM/Shell Sandbox)
   [ ] IPC          (Agent-to-Agent Swarm)
```

The wizard dynamically prompts based on selections:
- **Resources**: Picks `allowed_models` from your installed models (or `* (any installed model)`), and configures `max_tokens_per_minute`, Stateful Paging, and Memory Compaction limits (`max_json_tokens`).
- **Network**: Prompts to block data exfiltration (forces `allowed_methods = ["GET"]`).
- **Execution**: Configures WASM execution, `allowed_language_runtimes`, Raw Host Shell access, and whether this is a **coding/database agent** (`is_coding_agent`), which stops the firewall from blocking prompts that contain SQL or code.
- **IPC**: Configures Agent-to-Agent Swarm, Semantic Pipes, Semantic Persistence, and Time Decay.

Saves the manifest to `manifests/<app_id>.toml`. See [Manifest Reference](./manifest-reference.md).

---

### `ore clear <app_id>`

Wipe an agent's frozen SSD memory (swap page file).

```bash
ore clear my_agent
```

---

### `ore compact <app_id>`

Force a background memory compaction cycle for an agent. This parses the agent's chat history, mechanically summarizes it to fit within token limits, and evicts its stale KV-Cache to free up SSD space and GPU VRAM.

```bash
ore compact my_agent
```

---

### `ore kill <app_id>`

Emergency kill-switch for runaway agents: drops the agent's in-memory KV cache from the engine. Its frozen memory on disk is kept; use `ore clear` to wipe that too.

```bash
ore kill my_agent
```

---

## Tool Management

### `ore mktool <filepath> [flags]`

Compile a source file or an entire project directory into a secure WASM Cartridge or dynamic Memory Fusion plugin.

```bash
# 1. Standard Tools (Executed by Sandbox)
ore mktool script.py         # Compiles into ~/.ore/tools/script.wasm
ore mktool .                 # Compiles entire directory

# 2. Host Tools (Tools that load Memory Fusion plugins)
ore mktool host_agent.rs --host  # Surrenders table to the OS so kernel can hijack it

# 3. Dynamic Plugins (Loaded by Host Tools via ore_dlopen)
ore mktool encryptor.rs --shared  # Compiles into ~/.ore/plugins/encryptor.wasi.so
ore mktool . --shared             # Compiles entire directory

# Options
ore mktool tool.rs --name my_tool # Optional custom name for the output
ore mktool script.py --env data   # Python only: compiles with NumPy, Pandas, etc.
```

**Flags:**
| Flag | Description |
|---|---|
| *(No flag)* | Compiles as a standard WASM Tool. Outputs to `~/.ore/tools`. |
| `-H`, `--host` | Compiles as a WASM Tool that *loads plugins* via Memory Fusion. Outputs to `~/.ore/tools`. |
| `-s`, `--shared` | Compiles as a dynamically loadable plugin (`.wasi.so`) for Memory Fusion. Outputs to `~/.ore/plugins`. Rust, C, C++, and Zig only. |
| `-n`, `--name <name>` | Custom output name. Defaults to the filename or directory name. |
| `-e`, `--env <env>` | Python only: `pure` (RustPython AOT, ~25MB, default) or `data` (CPython 3.14 with Data Science libs, ~110MB). |

`--host` and `--shared` can't be combined. See [Building Plugins](./building-plugins.md) for what each flag passes to the compiler.

Supported languages: Rust (`.rs`), Go (`.go`), Python (`.py`), JavaScript (`.js`), TypeScript (`.ts`), Zig (`.zig`), C (`.c`), C++ (`.cpp`, `.cc`, `.cxx`).

**Toolchains:** Rust needs `cargo`/`rustup` (targets are added automatically). C, C++, and Zig are compiled with Zig (`zig cc` / `zig c++`). If Zig isn't available, `mktool` **installs it automatically** the first time it's needed: Zig 0.17.0 is downloaded from a randomly chosen community mirror (ziglang.org as the last fallback), its minisign signature is verified against the Zig Software Foundation key (including the signed file name, to block downgrades), and it's unpacked into `<ORE_DIR>/toolchains/zig-0.17.0/`. A `zig` already on your `PATH` is used instead, and `ORE_ZIG` can point at a specific binary. Python tools use the `system-py` runtimes, which are pulled automatically if missing.

**C++ note:** WASI's libc++ has no exception support, so C++ is compiled with `-fno-exceptions`. `try`/`catch` won't compile, and code that would throw (for example an out-of-range `vector::at`) aborts instead, which ends the tool with a trap.

> **Offline Toolchain:** `ore mktool` compiles cartridges completely offline and does not require `ore-server` to be running or an active security token.

---

**Next:** [API Reference →](./api-reference.md)
