# CLI Reference

> Every command the ORE CLI provides, with syntax, flags, and example output.

The CLI binary is `ore`. Install it with `cargo install --path ore-cli`.

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

# Output:
# ORE Kernel Status: ONLINE
# Engine: native
```

---

### `ore top`

View kernel telemetry - driver info, scheduler state, firewall status.

```bash
ore top

# Output:
# === ORE KERNEL TELEMETRY ===
# Driver      : Native Candle Engine
# Scheduler   : ACTIVE (Model: qwen2.5:0.5b, Users: 1)
# Firewall    : ARMED
# Memory GC   : cache_ttl=24h, pipe_ttl=32h
```

---

### `ore ps`

Show models currently loaded in GPU VRAM.

```bash
ore ps

# Output:
# MODEL                     | TOTAL RAM    | GPU VRAM
# ----------------------------------------------------------
# qwen2.5:0.5b              | 476       MB | 476       MB
```

---

### `ore ls`

List all locally installed models on disk.

```bash
ore ls

# Output:
# REPOSITORY                | SIZE       | UPDATED
# ------------------------------------------------------
# qwen2.5:0.5b              | 0.49 GB   | 2026-03-24 14:30:00
# llama3.2:1b               | 1.12 GB   | 2026-03-22 09:15:00
```

**Flags:**

| Flag | Description |
|---|---|
| `--agents` | List all registered agents with security status |
| `--manifests` | View raw permission matrix for all manifests |

```bash
ore ls --agents

# AGENT ID             | VERSION    | ALLOWED MODELS       | PRIORITY   | STATUS
# ----------------------------------------------------------------------------------
# openclaw             | 1.0.0      | llama3.2:1b          | NORMAL     | SECURED
# terminal_user        | 1.0.0      | llama3.2:1b          | NORMAL     | SECURED
# writer_agent         | 1.0.0      | llama3.2:1b          | NORMAL     | SECURED
# cyber_spider         | 1.0.0      | qwen2.5:0.5b, lla... | NORMAL     | UNSAFE
```

Status values:
- **SECURED** - PII redaction enabled, no shell access
- **UNSAFE** - Shell access granted or PII redaction disabled
- **DORMANT** - No models assigned

```bash
ore ls --manifests

# MANIFEST FILE        | NETWORK    | FILE I/O      | EXECUTION       | PII SCRUBBING
# ------------------------------------------------------------------------------------
# openclaw.toml        | ENABLED    | Read-Only     | WASM Sandbox    | ACTIVE
# terminal_user.toml   | BLOCKED    | Air-gapped    | Disabled        | ACTIVE
```

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
 ╭─ Secure Manifest Forage ───────────────────────────╮
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
- **Resources**: Configures `max_tokens_per_minute`, Stateful Paging, and Memory Compaction limits (`max_json_tokens`).
- **Network**: Prompts to block data exfiltration (forces `allowed_methods = ["GET"]`).
- **Execution**: Configures WASM execution, `allowed_language_runtimes`, and Raw Host Shell access.

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

Emergency kill-switch for runaway agents.

```bash
ore kill my_agent
```

---

## Tool Management

### `ore mk-tool <filepath>`

Compile a source file into a secure WASM Cartridge.

```bash
ore mk-tool script.py
```

Supported languages: Rust (`.rs`), Go (`.go`), Python (`.py`), JavaScript (`.js`), TypeScript (`.ts`), Zig (`.zig`), C (`.c`), C++ (`.cpp`, `.cc`, `.cxx`).
The resulting `.wasm` file can be executed safely by any agent in the Zero-Trust WASM Sandbox.

---

**Next:** [API Reference →](./api-reference.md)
