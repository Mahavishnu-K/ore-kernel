# API Reference

> All 19 authenticated HTTP routes exposed by the Ore kernel daemon.

**Base URL:** `http://127.0.0.1:6767`

**Authentication:** Every request requires a `Authorization: Bearer <token>` header. The token is auto-generated on boot and written to `ore-kernel.token`. Unauthorized requests receive `401 UNAUTHORIZED`.

**Response formats:** `/health`, `/ps`, `/ls`, `/top`, `/agents`, and `/manifests` return **JSON** when the request sends `Accept: application/json`, and a human-readable text table otherwise. The CLI uses the JSON form. Other routes return plain text unless noted.

---

## System Routes

Source: [`ore-server/src/handlers/system.rs`](../ore-server/src/handlers/system.rs)

### `GET /health`

Kernel health check.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Accept: application/json" \
     http://127.0.0.1:6767/health
```

**JSON response** (`KernelHealthResponse`):
```json
{
  "status": "ONLINE",
  "version": "0.1.0",
  "engine": "Native Candle Engine",
  "device": "Host CPU (SIMD)",
  "ore_dir": "/home/user/.ore",
  "loaded_models": 1,
  "registered_agents": 8,
  "uptime_seconds": 5321
}
```

Without the `Accept` header, the same information comes back as text starting with `ORE Kernel is ALIVE & HEALTHY (v0.1.0)`.

---

### `GET /ps`

List models currently loaded in memory.

**JSON response** (`PsResponse`):
```json
{
  "models": [
    {
      "model_name": "qwen2.5:0.5b",
      "engine": "Native Candle Engine",
      "device": "Host CPU (SIMD)",
      "status": "IDLE",
      "active_requests": 0,
      "host_ram_mb": 500,
      "gpu_vram_mb": 0,
      "last_used_secs": 12,
      "current_app_id": "openclaw"
    }
  ],
  "total_ram_mb": 500,
  "total_vram_mb": 0
}
```

---

### `GET /ls`

List all models installed on disk.

**JSON response** (`LsResponse`):
```json
{
  "models": [
    {
      "name": "qwen2.5:0.5b",
      "format": "GGUF (Quantized)",
      "size_bytes": 491400032,
      "modified_at": "2026-03-24 14:30:00",
      "is_loaded": true
    }
  ]
}
```

---

### `GET /top`

System telemetry and subsystem status (what `ore top` displays).

**JSON response** (`TopTelemetryResponse`): host telemetry (`host`), the accelerator and its VRAM figures (`accelerator_device`, `vram_total_mb`, `vram_used_mb`, `vram_free_mb`, `vram_reserved_mb`, `vram_safety_margin_mb`), and a status line per subsystem (`engine_name`, `scheduler_summary`, `active_models_count`, `registered_agents_count`, `context_firewall`, `semantic_bus_cache_count`, `wasm_sandbox`). See `ore-common/src/protocol.rs` for the exact schema.

---

### `GET /kill/:app_id`

Emergency stop for an agent's inference state: drops the agent's in-memory KV cache from the engine. Frozen memory on disk is kept (use `/clear` to wipe it).

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:6767/kill/cyber_spider
```

**Response:**
```text
SUCCESS: App 'cyber_spider' context wiped from GPU Memory.
```

---

### `GET /agents`

Agent security dashboard: every registered agent with its permissions and a status label.

**JSON response** (`AgentsResponse`):
```json
{
  "agents": [
    {
      "app_id": "cyber_spider",
      "version": "1.0.0",
      "allowed_models": ["qwen2.5:0.5b"],
      "priority": "normal",
      "network_enabled": true,
      "filesystem_permissions": "Read-Only",
      "execution_engine": "Shell (Risk)",
      "pii_enforcement": false,
      "status": "UNSAFE"
    }
  ]
}
```

`status` is `UNSAFE` (shell access), `AIR-GAPPED` (no network), or `SECURED`. `filesystem_permissions` is `Read/Write`, `Read-Only`, `Write-Only`, or `Isolated`; `execution_engine` is `Shell (Risk)`, `WASM Sandbox`, or `Prompt-Only`.

---

### `GET /manifests`

Raw permission matrix for every manifest file.

**JSON response** (`ManifestsResponse`): a `manifests` array of `{ file_name, app_id, network, file_io, execution, pii_scrubbing }` strings.

---

### `GET /pull/:model`

Install a model through the active driver.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:6767/pull/qwen2.5:0.5b
```

**Response:**
```text
SUCCESS: Model 'qwen2.5:0.5b' installed.
```

With the **Ollama** engine this runs `ollama pull`. With the **native** engine it does nothing and reports success: use `ore pull <model>`, which downloads from Hugging Face directly.

---

### `GET /load/:model`

Pre-load a model so the first request doesn't pay the load time.

**Response:** `SUCCESS: Model 'qwen2.5:0.5b' loaded.` or `KERNEL ERROR: …`

---

### `GET /expel/:model`

Force-evict a model from memory.

**Response:** `SUCCESS: Model 'qwen2.5:0.5b' has been forcefully evicted from GPU VRAM.` or `KERNEL ERROR: …`

---

### `GET /clear/:app_id`

Wipe an agent's frozen memory (chat history and saved KV cache) from SSD and RAM.

**Response:** `SUCCESS: Memory for Agent 'my_agent' has been wiped clean from SSD and RAM.`

---

### `GET /compact/:app_id`

Force a memory compaction cycle for an agent: the model summarizes the chat history into a dense fact list, and the stale physical KV cache is discarded. Requires `json_history = true` in the agent's manifest.

**Response:** `SUCCESS: Memory for Agent 'my_agent' manually compacted.`, or `KERNEL ERROR: …` (unregistered agent, no JSON history) / `ORE KERNEL ALERT: GPU unavailable - …`

---

### `POST /execute`

Executes a pre-compiled `.wasm` tool (**Tool Mode**), runs an autonomous script (**Script Mode** / Inception), OR executes a raw host shell command (**Shell Mode**).

Source: [`ore-server/src/execution/mod.rs`](../ore-server/src/execution/mod.rs)

#### 1. Tool Mode ("Console Cartridges")
Invokes a pre-compiled WASM binary, `~/.ore/tools/<tool_name>.wasm`. Requires `can_execute_wasm = true` and the tool listed in `allowed_tools`.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{
       "app_id": "openclaw",
       "tool_name": "file_search",
       "args": ["--path", "/workspace"],
       "input_data": "optional STDIN data"
     }' \
     http://127.0.0.1:6767/execute
```

#### 2. Script Mode ("Inception")
Materializes and runs dynamic Python or JavaScript scripts inside the WASM sandbox using pre-compiled runtimes (`system-py.wasm`, `system-js.wasm`). Requires language in `allowed_language_runtimes`. Supports transparent dependencies handling.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{
       "app_id": "wasm_agent",
       "language": "python",
       "script": "import requests\nr = requests.get(\"https://httpbin.org/get\")\nprint(r.status_code)",
       "dependencies": ["requests"],
       "input_data": "optional STDIN data"
     }' \
     http://127.0.0.1:6767/execute
```

#### 3. Shell Mode (Raw Host Shell Bypass)
Executes a raw host command directly. Requires `can_execute_shell = true` in the agent's manifest. **⚠️ Flags the agent as UNSAFE.**

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{
       "app_id": "terminal_user",
       "shell_command": "git status --short"
     }' \
     http://127.0.0.1:6767/execute
```

#### Schema Reference

| Field | Type | Required | Description |
|---|---|---|---|
| `app_id` | string | ✅ | Registered App ID defining manifest permissions |
| `tool_name` | string | ❌ (Tool Mode) | Name of pre-compiled tool binary in `tools/` (without `.wasm`) |
| `args` | string[] | ❌ (Tool Mode) | CLI arguments passed to the WASM cartridge |
| `input_data` | string | ❌ (Tool/Script) | Complex text or JSON passed into the sandbox via STDIN |
| `language` | string | ❌ (Script Mode) | Language runtime: `"python"`, `"js"`, `"javascript"`, `"ts"` |
| `script` | string | ❌ (Script Mode) | Script code content to dynamically materialize and execute |
| `dependencies` | string[] | ❌ (Script Mode) | List of package dependencies (e.g., `["requests", "numpy"]`) |
| `shell_command` | string | ❌ (Shell Mode) | Host shell command to execute directly on the host OS |

> **Mode Selection Rule:** Exactly one mode must be provided per request (`tool_name`, `script`, or `shell_command`). Ambiguous or empty requests return a descriptive error.

**Response:** plain text. The tool's stdout, then `--- STDERR ---` and its stderr if any (each capped at 10 MiB). If the run failed, the output ends with exactly one `KERNEL ERROR:` line:

| Outcome | Last line of the response |
|---|---|
| Success (returned, or exited with status 0) | *(nothing extra)* |
| Exited with a non-zero status (Go panic, uncaught Python exception, `exit(1)`) | `KERNEL ERROR: Guest exited with status N` |
| Fuel limit reached | `KERNEL ERROR: Sandbox Trap: CPU Fuel Exhausted (Runaway AI or Infinite Loop Detected)` |
| Any other trap (panic, abort, out-of-bounds access, stale plugin pointer) | `KERNEL ERROR: <trap message>` |

Requests rejected before the tool runs (unregistered agent, tool not in `allowed_tools`, missing `.wasm`, ambiguous mode) return a single error line instead, such as `KERNEL ERROR: Tool 'x' is not whitelisted in manifest. Add it to allowed_tools.` To detect any failure, check whether the response contains `KERNEL ERROR`.

---


## Inference Routes

Source: [`ore-server/src/handlers/inference.rs`](../ore-server/src/handlers/inference.rs)

### `GET /ask/:prompt`

Secured inference with full firewall pipeline + SSD paging. Always runs as the **`openclaw`** agent (its manifest and memory), using the first model in its `allowed_models`. Underscores in the prompt are turned into spaces.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:6767/ask/What%20is%20a%20mutex
```

**Pipeline:** Auth → Manifest lookup → Firewall (injection + PII) → Page-in history & KV-Cache → Scheduler → Inference → Page-out history & KV-Cache (with Compaction support)

---

### `POST /run`

Streamed inference with rate limiting, firewall, and scheduler admission.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{"app_id": "my_agent", "model": "qwen2.5:0.5b", "prompt": "Explain ownership in Rust"}' \
     http://127.0.0.1:6767/run
```

**Request Body:**
```json
{
  "app_id": "my_agent",
  "model": "qwen2.5:0.5b",
  "prompt": "Explain ownership in Rust"
}
```

**Response:** a chunked plain-text stream of tokens, written as they're generated.

**Errors:** `401` for an unregistered `app_id`, `429` when the agent's rate limit is exceeded (each request is charged 1,000 tokens), `403` with `ORE KERNEL ALERT: SECURITY BREACH …` when the firewall blocks the prompt, and `503` when the scheduler can't admit the request.

The `model` field isn't checked against the agent's `allowed_models` yet.

---

## IPC Routes

Source: [`ore-server/src/handlers/ipc.rs`](../ore-server/src/handlers/ipc.rs)

### `POST /ipc/share`

Write knowledge to a Semantic Bus pipe. The text is chunked, embedded, and stored in the in-memory vector database.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{
       "source_app": "writer_agent",
       "target_pipe": "rust_docs",
       "knowledge_text": "Rust ownership means each value has exactly one owner...",
       "chunk_size": 100,
       "chunk_overlap": 20,
       "chunk_strategy": "sentence_aware"
     }' \
     http://127.0.0.1:6767/ipc/share
```

**Request Body:**
```json
{
  "source_app": "writer_agent",
  "target_pipe": "rust_docs",
  "knowledge_text": "The text to embed and store",
  "chunk_size": 100,
  "chunk_overlap": 20,
  "chunk_strategy": "sliding_window"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `source_app` | string | ✅ | The app writing the knowledge (must have pipe in `allowed_semantic_pipes`) |
| `target_pipe` | string | ✅ | Named pipe to write to |
| `knowledge_text` | string | ✅ | Raw text to chunk and embed |
| `chunk_size` | usize | ❌ | Chunk size (default: 100) |
| `chunk_overlap` | usize | ❌ | Overlap between chunks (default: 20). If it's ≥ `chunk_size`, `chunk_size / 4` is used instead |
| `chunk_strategy` | string | ❌ | Strategy to use: `"sliding_window"` (default), `"sentence_aware"`, `"paragraph"`, `"exact_match"` |

**Response:** plain text, `SUCCESS: …` or a `KERNEL ALERT` / `KERNEL ERROR` line (unregistered agent, pipe not in `allowed_semantic_pipes`, embedding failure).

---

### `POST /ipc/search`

Search a Semantic Bus pipe using fast dot-product similarity with time-decay scoring.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{
       "source_app": "writer_agent",
       "target_pipe": "rust_docs",
       "query": "How does ownership work?",
       "top_k": 3
     }' \
     http://127.0.0.1:6767/ipc/search
```

**Request Body:**
```json
{
  "source_app": "writer_agent",
  "target_pipe": "rust_docs",
  "query": "Search query text",
  "filter_app": null,
  "top_k": 3
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `source_app` | string | ✅ | The app performing the search (must have pipe in `allowed_semantic_pipes`) |
| `target_pipe` | string | ✅ | Named pipe to search |
| `query` | string | ✅ | Natural-language search query |
| `filter_app` | string | ❌ | Only return results from this specific app |
| `top_k` | usize | ❌ | Number of results (default: 5) |

**Response:** Array of `SearchResult` JSON objects containing the top-K most relevant text chunks, ranked by dot-product match × time-decay factor. Example:
```json
[
  {
    "text": "Rust ownership means each value...",
    "score": 0.98,
    "source_app": "scraper_agent",
    "timestamp": 1711728200
  }
]
```

**Errors:** Returns standard HTTP status codes like `401 Unauthorized` or `403 Forbidden` if manifest permissions are denied.

---

### `POST /ipc/send`

Send a direct message from one agent to another via the Message Bus.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{
       "from_app": "openclaw",
       "to_app": "writer_agent",
       "payload": "Please summarize the latest Rust RFC"
     }' \
     http://127.0.0.1:6767/ipc/send
```

The sender must have the target listed in `allowed_agent_targets` in its manifest.

**Response:** `SUCCESS: Message delivered.`, or a `KERNEL ALERT` / `KERNEL ERROR` line. Each agent's inbox holds up to **1,024** pending messages; when it's full, new messages are rejected with `Target agent '…' queue is full (capacity: 1024). Backpressure applied.`

---

### `GET /ipc/listen/:app_id`

Poll for the next incoming message. No registration is needed: an agent's inbox is created the first time a message is sent to it. The `app_id` must be registered.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:6767/ipc/listen/writer_agent
```

**Response:** JSON, either the next pending message or `null` if the inbox is empty (or the `app_id` is unknown):

```json
{
  "from_app": "openclaw",
  "to_app": "writer_agent",
  "payload": "Please summarize the latest Rust RFC",
  "timestamp": 1711728200
}
```

---

**Next:** [Manifest Reference →](./manifest-reference.md)
