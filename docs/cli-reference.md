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
# MODEL              SIZE        VRAM
# qwen2.5:0.5b       500MB       500MB
```

---

### `ore ls`

List all locally installed models on disk.

```bash
ore ls

# Output:
# MODEL              SIZE        MODIFIED
# qwen2.5:0.5b       500MB       2026-03-24 14:30
# llama3.2:1b        1.2GB       2026-03-22 09:15
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
ore pull llama3.2:1b

# Safetensors (full-precision, for embeddings)
ore pull system-embedder
```

All downloads stream directly to `models/` with zero RAM bloat. Supports HuggingFace token for gated models.

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

### `ore run <model> <prompt>`

Execute a secured inference request with streamed output.

```bash
ore run qwen2.5:0.5b "Explain what a semaphore is"
```

The prompt passes through the full firewall pipeline (injection detection → PII redaction → boundary enforcement) before reaching the model.

---

## Agent Management

### `ore manifest <app_id>`

Interactive wizard to generate a secure `.toml` manifest.

```bash
ore manifest my_agent
```

```
 ORE KERNEL :: SECURE MANIFEST FORGE
 Target agent :: my_agent

 Select all the required sub-systems:
  [ ] Privacy      [ PII Redaction ]
  [ ] Resources    [ GPU Quotas & Models ]
  [ ] File System  [ File System Boundaries ]
  [ ] Network      [ Network Egress Control ]
  [ ] Execution    [ WASM/Shell Sandbox ]
  [ ] IPC          [ Agent-to-Agent Swarm ]
```

Saves the manifest to `manifests/<app_id>.toml`. See [Manifest Reference](./manifest-reference.md).

---

### `ore clear <app_id>`

Wipe an agent's frozen SSD memory (swap page file).

```bash
ore clear my_agent
```

---

### `ore kill <app_id>`

Emergency kill-switch for runaway agents.

```bash
ore kill my_agent
```

---

**Next:** [API Reference →](./api-reference.md)
