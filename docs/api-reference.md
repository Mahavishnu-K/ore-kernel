# API Reference

> All 15 authenticated HTTP routes exposed by the ORE kernel daemon.

**Base URL:** `http://127.0.0.1:3000`

**Authentication:** Every request requires a `Authorization: Bearer <token>` header. The token is auto-generated on boot and written to `ore-kernel.token`. Unauthorized requests receive `401 UNAUTHORIZED`.

---

## System Routes

Source: [`ore-server/src/handlers/system.rs`](../ore-server/src/handlers/system.rs)

### `GET /health`

Kernel health check. Returns the active engine name.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/health
```

**Response:**
```json
"ORE Kernel Online | Engine: native"
```

---

### `GET /ps`

List models currently loaded in GPU VRAM.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/ps
```

**Response:**
```json
[
  {
    "model_name": "qwen2.5:0.5b",
    "size_bytes": 524288000,
    "size_vram_bytes": 524288000
  }
]
```

---

### `GET /ls`

List all locally installed models on disk.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/ls
```

**Response:**
```json
[
  {
    "name": "qwen2.5:0.5b",
    "size_bytes": 524288000,
    "modified_at": "2026-03-24 14:30"
  }
]
```

---

### `GET /agents`

Agent security dashboard - lists all registered agents with security assessment.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/agents
```

---

### `GET /manifests`

Raw permission matrix for all registered manifests.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/manifests
```

---

### `GET /pull/:model`

Download and install a model (triggers HuggingFace or Ollama pull).

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/pull/qwen2.5:0.5b
```

---

### `GET /load/:model`

Pre-load a model into VRAM for zero-latency inference.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/load/qwen2.5:0.5b
```

---

### `GET /expel/:model`

Force-evict a model from GPU VRAM.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/expel/qwen2.5:0.5b
```

---

### `GET /clear/:app_id`

Wipe an agent's SSD swap memory (frozen context).

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/clear/my_agent
```

---

## Inference Routes

Source: [`ore-server/src/handlers/inference.rs`](../ore-server/src/handlers/inference.rs)

### `GET /ask/:prompt`

Secured inference with full firewall pipeline + SSD paging. Uses `terminal_user` manifest by default.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/ask/What%20is%20a%20mutex
```

**Pipeline:** Auth → Manifest lookup → Firewall (injection + PII + boundary) → Page-in history → Scheduler → Inference → Page-out history

---

### `POST /run`

Streamed inference with rate limiting. Returns tokens as a `text/event-stream`.

```bash
curl -X POST \
     -H "Authorization: Bearer $(cat ore-kernel.token)" \
     -H "Content-Type: application/json" \
     -d '{"model": "qwen2.5:0.5b", "prompt": "Explain ownership in Rust"}' \
     http://127.0.0.1:3000/run
```

**Request Body:**
```json
{
  "model": "qwen2.5:0.5b",
  "prompt": "Explain ownership in Rust"
}
```

**Response:** `text/event-stream` - tokens streamed in real-time.

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
       "chunk_size": 50,
       "chunk_overlap": 10
     }' \
     http://127.0.0.1:3000/ipc/share
```

**Request Body:**
```json
{
  "source_app": "writer_agent",
  "target_pipe": "rust_docs",
  "knowledge_text": "The text to embed and store",
  "chunk_size": 50,
  "chunk_overlap": 10
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `source_app` | string | ✅ | The app writing the knowledge (must have pipe in `allowed_semantic_pipes`) |
| `target_pipe` | string | ✅ | Named pipe to write to |
| `knowledge_text` | string | ✅ | Raw text to chunk and embed |
| `chunk_size` | usize | ❌ | Words per chunk (default: 50) |
| `chunk_overlap` | usize | ❌ | Overlap words between chunks (default: 10) |
| `chunk_strategy` | string | ❌ | Strategy to use: `"sliding_window"` (default), `"sentence_aware"`, `"paragraph"`, `"exact_match"` |

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
     http://127.0.0.1:3000/ipc/search
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
| `top_k` | usize | ❌ | Number of results (default: 3) |

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
     http://127.0.0.1:3000/ipc/send
```

The sender must have the target listed in `allowed_agent_targets` in its manifest.

---

### `GET /ipc/listen/:app_id`

Poll for incoming agent messages. The agent must first register as a listener.

```bash
curl -H "Authorization: Bearer $(cat ore-kernel.token)" \
     http://127.0.0.1:3000/ipc/listen/writer_agent
```

**Response:** The next pending `AgentMessage` from the unbounded channel queue, or empty if none.

---

**Next:** [Manifest Reference →](./manifest-reference.md)
