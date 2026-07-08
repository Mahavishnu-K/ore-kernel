# Manifest Reference

> The AppManifest is ORE's permission contract. Every agent must have one.

## Overview

An AppManifest is a `.toml` file in the `manifests/` directory that declares exactly what an agent is allowed to do. ORE enforces these permissions at the kernel level - not in the application. No manifest = no access.

Generate one interactively:

```bash
ore manifest my_agent
```

Source: [`ore-core/src/registry.rs`](../ore-core/src/registry.rs)

---

## Full Schema

```toml
# Required fields
app_id = "my_agent"
description = "My agent description"
version = "1.0.0"

# ─── Privacy ─────────────────────────────────────
[privacy]
enforce_pii_redaction = true          # Scrub emails + credit cards before inference

# ─── Resources ───────────────────────────────────
[resources]
allowed_models = ["llama3.2:1b", "qwen2.5:0.5b"]
max_tokens_per_minute = 10000         # Rate limit enforced by the kernel
gpu_priority = "normal"               # "low", "normal", "high"
json_history = false                  # Use JSON text fallback for memory compaction
stateful_paging = true                # Enable SSD context swap for long conversations

# ─── Memory Limits ───────────────────────────────
[memory_limits]
max_json_tokens = 8192                # Max conversational context tokens before summarization
max_kv_cache_mb = 1024                # SSD/VRAM size limit for frozen state
auto_summarize_on_cap = true          # Summarize history when limits hit

# ─── File System ─────────────────────────────────
[file_system]
allowed_read_paths = ["/home/user/projects"]
allowed_write_paths = []
max_file_size_mb = 5

# ─── Network ─────────────────────────────────────
[network]
network_enabled = true
allowed_domains = ["github.com", "docs.rs"]
allow_localhost_access = false

# ─── Execution ───────────────────────────────────
[execution]
can_execute_shell = false             # ⚠️ High risk - flagged as UNSAFE
can_execute_wasm = true
allowed_tools = ["file_search", "git_commit"]

# ─── IPC ─────────────────────────────────────────
[ipc]
allowed_agent_targets = ["writer_agent"]     # Tier 1: Direct messaging
allowed_semantic_pipes = ["rust_docs"]       # Tier 2: Semantic memory access
semantic_persistence = false                 # Freeze semantic pipes to SSD
```

---

## Field Reference

### Root Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `app_id` | string | ✅ | Unique identifier for this agent |
| `description` | string | ✅ | Human-readable description |
| `version` | string | ✅ | Semantic version string |

### `[privacy]`

| Field | Type | Default | Description |
|---|---|---|---|
| `enforce_pii_redaction` | bool | `false` | When `true`, the firewall scrubs emails and credit card numbers from prompts before inference |

### `[resources]`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_models` | string[] | `[]` | Models this agent is permitted to use. Inference requests for unlisted models are rejected |
| `max_tokens_per_minute` | u32 | `0` | Token rate limit per 60-second window. `0` = unlimited |
| `gpu_priority` | string | `""` | Scheduling priority: `"low"`, `"normal"`, `"high"` |
| `json_history` | bool | `false` | When `true`, retains chat history as JSON text to prevent KV-cache corruption |
| `stateful_paging` | bool | `false` | When `true`, Memory Management freezes and restores chat history across requests |

### `[memory_limits]`

| Field | Type | Default | Description |
|---|---|---|---|
| `max_json_tokens` | u32 | `8192` | Maximum conversational context in tokens. (Heuristic: 1 token ~= 4 chars of JSON text) |
| `max_kv_cache_mb` | u32 | `1024` | The physical SSD/VRAM size limit for the frozen brain state in MB |
| `auto_summarize_on_cap` | bool | `true` | If limits are hit, whether ORE should summarize the history |

### `[file_system]`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_read_paths` | string[] | `[]` | File system paths the agent can read from |
| `allowed_write_paths` | string[] | `[]` | File system paths the agent can write to |
| `max_file_size_mb` | u32 | `0` | Maximum file size the agent may access |

### `[network]`

| Field | Type | Default | Description |
|---|---|---|---|
| `network_enabled` | bool | `false` | Whether the agent has any network access |
| `allowed_domains` | string[] | `[]` | Domain allowlist for outbound connections |
| `allow_localhost_access` | bool | `false` | Whether the agent can reach `127.0.0.1` / `localhost` |

### `[execution]`

| Field | Type | Default | Description |
|---|---|---|---|
| `can_execute_shell` | bool | `false` | Whether shell execution is allowed. **⚠️ Flagged as UNSAFE** in `ore ls --agents` |
| `can_execute_wasm` | bool | `false` | Whether WASM sandboxed execution is allowed |
| `allowed_tools` | string[] | `[]` | Named tools this agent may invoke |

### `[ipc]`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_agent_targets` | string[] | `[]` | Agent IDs this agent can send direct messages to via the Message Bus |
| `allowed_semantic_pipes` | string[] | `[]` | Named semantic pipes this agent can read from and write to |
| `semantic_persistence` | bool | `false` | When `true`, the Pager will save semantic pipes to the SSD using Bincode serialization |

---

## Validation Rules

The ORE Kernel enforces strict sanity checks on manifests at boot:

- **Immutable Anchor:** `stateful_paging` cannot be `true` if `json_history` is `false`. JSON fallbacks are required for memory compaction.
- **Non-Zero Budgets:** `max_tokens_per_minute` cannot be `0`.
- **Memory Context:** If `json_history` is true, `max_json_tokens` must be at least `500`.
- **VRAM Space:** If `stateful_paging` is true, `max_kv_cache_mb` cannot be `0`.

---

## Security Status Rules

The `ore ls --agents` command flags each agent based on its manifest:

| Status | Condition |
|---|---|
| **SECURED** | PII redaction enabled AND no shell access |
| **UNSAFE** | Shell access granted OR PII redaction disabled |
| **DORMANT** | No models assigned (`allowed_models` is empty) |

---

## Examples

### Minimal Manifest (Air-gapped Agent)

```toml
app_id = "sandbox_agent"
description = "Fully isolated agent"
version = "1.0.0"

[privacy]
enforce_pii_redaction = true

[resources]
allowed_models = ["qwen2.5:0.5b"]
max_tokens_per_minute = 5000
gpu_priority = "normal"
```

### Collaborative Agent (IPC Enabled)

```toml
app_id = "writer_agent"
description = "Agent that writes and shares knowledge"
version = "1.0.0"

[privacy]
enforce_pii_redaction = true

[resources]
allowed_models = ["llama3.2:1b"]
max_tokens_per_minute = 10000
gpu_priority = "normal"
json_history = false
stateful_paging = false

[ipc]
allowed_agent_targets = ["terminal_user"]
allowed_semantic_pipes = ["rust_docs"]
semantic_persistence = false
```

### Power User Agent (Network + File Access)

```toml
app_id = "research_agent"
description = "Agent with web and file access"
version = "1.0.0"

[privacy]
enforce_pii_redaction = true

[resources]
allowed_models = ["llama3.2:1b", "qwen2.5:0.5b"]
max_tokens_per_minute = 20000
gpu_priority = "high"
json_history = true
stateful_paging = true

[memory_limits]
max_json_tokens = 8192
max_kv_cache_mb = 1024
auto_summarize_on_cap = true

[file_system]
allowed_read_paths = ["/home/user/research"]
allowed_write_paths = ["/home/user/research/output"]
max_file_size_mb = 10

[network]
network_enabled = true
allowed_domains = ["arxiv.org", "docs.rs", "github.com"]
allow_localhost_access = false

[execution]
can_execute_shell = false
can_execute_wasm = true
allowed_tools = ["file_search", "web_fetch"]

[ipc]
allowed_agent_targets = ["writer_agent"]
allowed_semantic_pipes = ["research_papers", "rust_docs"]
semantic_persistence = true
```

---

**Next:** [Security Model →](./security-model.md)
