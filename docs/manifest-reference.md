# Manifest Reference

> The AppManifest is Ore's permission contract. Every agent must have one.

## Overview

An AppManifest is a `.toml` file in the `manifests/` directory that declares what an agent is allowed to do. Ore enforces these permissions in the kernel, not in the application. No manifest = no access.

Generate one interactively:

```bash
ore manifest my_agent
```

**Schema:** [`ore-common/src/manifest.rs`](../ore-common/src/manifest.rs) · **Loader:** [`ore-core/src/registry.rs`](../ore-core/src/registry.rs)

Manifests are loaded **once, at boot**. Restart the kernel after adding or editing one.

---

## Full Schema

```toml
# Required fields
app_id = "my_agent"
description = "My agent description"
version = "1.0.0"

# ─── Privacy ─────────────────────────────────────
[privacy]
enforce_pii_redaction = true          # Run the DLP redactor on prompts before inference

# ─── Resources ───────────────────────────────────
[resources]
allowed_models = ["llama3.2:1b", "qwen2.5:0.5b"]   # or ["*"] for any installed model
max_tokens_per_minute = 10000         # Rate limit enforced by the kernel
gpu_priority = "normal"               # "low", "normal", "high" (not yet used by the scheduler)
json_history = true                   # Keep chat history as JSON (required for paging)
stateful_paging = true                # Page the KV cache to SSD between turns

# ─── Memory Limits ───────────────────────────────
[memory_limits]
max_json_tokens = 8192                # Max conversational context tokens before compaction
max_kv_cache_mb = 1024                # Size limit for the frozen KV cache
auto_summarize_on_cap = true          # Summarize history when limits are hit

# ─── File System ─────────────────────────────────
[file_system]
allowed_read_paths = ["/home/user/projects"]   # Directories only
allowed_write_paths = []

# ─── Network ─────────────────────────────────────
[network]
network_enabled = true
allow_localhost_access = false

[[network.rules]]
domain = "api.github.com"              # Exact host match, or "*"
allowed_methods = ["GET", "POST"]      # Specific HTTP verbs, or ["*"]
allowed_paths = ["/repos*"]            # "*", a "/prefix*", or an exact path (and below)

# ─── Execution ───────────────────────────────────
[execution]
can_execute_shell = false             # ⚠️ High risk: flagged as UNSAFE
can_execute_wasm = true
is_coding_agent = false               # Let code/SQL text pass the injection firewall
allowed_tools = ["file_search", "git_commit"] # Use ["*"] to allow all tools
allowed_language_runtimes = ["python", "js"]  # Use ["*"] to allow all runtimes
max_cpu_instructions = 5000000000     # Fuel limit (default: 5 billion instructions)

# ─── IPC ─────────────────────────────────────────
[ipc]
allowed_agent_targets = ["writer_agent"]     # Tier 1: Direct messaging
allowed_semantic_pipes = ["rust_docs"]       # Tier 2: Semantic memory access
semantic_persistence = true                  # Freeze semantic pipes to SSD
allow_time_decay = false                     # Older memories lose relevance in search
```

---

## Field Reference

**Defaults:** every section (`[privacy]`, `[resources]`, …) can be left out entirely, and then gets the defaults below. If you **do** write a section, the fields marked **required in section** must be present, or the manifest fails to parse.

### Root Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `app_id` | string | ✅ | Unique identifier for this agent. Must not be empty |
| `description` | string | ✅ | Human-readable description |
| `version` | string | ✅ | Version string |

### `[privacy]`

| Field | Type | Default | Description |
|---|---|---|---|
| `enforce_pii_redaction` | bool | `false` · required in section | When `true`, the firewall redacts emails, phone numbers, SSNs, card numbers, IBANs, API keys, private keys, and internal IPs from prompts. See [Context Firewall](./kernel-internals/context-firewall.md) |

### `[resources]`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_models` | string[] | `[]` · required in section | Models this agent may run. `/run` rejects any other model with `403`. `"*"` allows every installed model. `:` and `-` match each other (`qwen2.5:0.5b` = `qwen2.5-0.5b`). `/infer` and `/compact` use the first entry that isn't `"*"` |
| `max_tokens_per_minute` | u32 | `10000` · required in section | Rate limit per 60-second window. Must not be `0`. `/run` charges a flat 1,000 tokens per request |
| `gpu_priority` | string | `"normal"` · required in section | `"low"`, `"normal"`, or `"high"`. Shown in dashboards; not yet used by the scheduler |
| `json_history` | bool | `true` | Keep chat history as JSON text. Required for paging and compaction |
| `stateful_paging` | bool | `false` | Page the agent's physical KV cache to SSD between turns and restore it on the next request |

### `[memory_limits]`

| Field | Type | Default | Description |
|---|---|---|---|
| `max_json_tokens` | u32 | `8192` · required in section | Maximum conversational context, in tokens, before compaction |
| `max_kv_cache_mb` | u32 | `1024` · required in section | Size limit for the agent's frozen KV cache, in MB |
| `auto_summarize_on_cap` | bool | `true` · required in section | Summarize the history when a limit is reached |

### `[file_system]`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_read_paths` | string[] | `[]` · required in section | Host **directories** mounted read-only into the sandbox under `/workspace` |
| `allowed_write_paths` | string[] | `[]` · required in section | Host **directories** mounted read-write under `/workspace` |

WASI can only mount directories, not individual files.

### `[network]`

| Field | Type | Default | Description |
|---|---|---|---|
| `network_enabled` | bool | `false` · required in section | Whether the agent has any network access. Also lets network-probe text through the firewall |
| `allow_localhost_access` | bool | `false` · required in section | Whether sandboxed tools can reach this machine: `localhost`, `127.0.0.0/8`, `0.0.0.0`, `::1`, and link-local addresses such as the cloud metadata IP `169.254.169.254` |
| `rules` | NetworkRule[] | `[]` | `[[network.rules]]` entries. See below |

**`[[network.rules]]`** (all three fields required per rule):

| Field | Type | Description |
|---|---|---|
| `domain` | string | Host that must **exactly** equal the URL's host (`api.github.com` doesn't cover `github.com`), or `"*"` for any host |
| `allowed_methods` | string[] | HTTP methods allowed for that host, or `["*"]` |
| `allowed_paths` | string[] | Paths allowed on that host: `"*"` for all, `"/prefix*"` for a prefix, or a plain path for that path and everything below it (`"/api"` covers `/api/v1` but not `/apiary`). An empty list allows nothing |

### `[execution]`

| Field | Type | Default | Description |
|---|---|---|---|
| `can_execute_shell` | bool | `false` | Allows raw host shell commands, bypassing the sandbox. **⚠️ Marks the agent UNSAFE**, and makes the firewall block any matched threat regardless of severity |
| `can_execute_wasm` | bool | `true` | Allows sandboxed WASM execution (`/execute` tools and scripts) |
| `is_coding_agent` | bool | `false` | Lets SQL and code-execution text (`DROP TABLE`, `eval(`, `os.system`) through the injection firewall, for agents that legitimately discuss code |
| `allowed_tools` | string[] | `[]` | Tool names (from `~/.ore/tools/<name>.wasm`) this agent may run. `["*"]` allows all |
| `allowed_language_runtimes` | string[] | `[]` | Script languages for Inception (script) mode. `["*"]` allows all |
| `max_cpu_instructions` | u64 | `5000000000` | Fuel limit per WASM execution (5 billion ≈ 2–3 s of pure compute) |

### `[ipc]`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_agent_targets` | string[] | `[]` | Agent IDs this agent can message via the Message Bus |
| `allowed_semantic_pipes` | string[] | `[]` | Semantic pipes this agent can read from and write to |
| `semantic_persistence` | bool | `true` | Save this agent's semantic pipes to SSD (Bincode) |
| `allow_time_decay` | bool | `false` | Older memories gradually lose relevance in semantic search |

---

## Loading and Validation

At boot, the registry reads every `.toml` file in `manifests/`:

- **Bad manifests are skipped, never fatal.** A file with invalid TOML, a wrong type, or a missing required field is logged as `[SECURITY ALERT] Skipping manifest '<file>'`, and the kernel boots without that agent.
- **Validation errors skip the manifest** the same way (`[SECURITY ALERT] Failed to load manifest for '<app_id>'`).
- **Duplicate `app_id`s:** the first file loaded wins, and later ones are skipped with a warning.

Validation rules (`AppManifest::validate`):

- **Non-empty ID:** `app_id` cannot be blank.
- **Immutable anchor:** `stateful_paging` cannot be `true` if `json_history` is `false`. JSON history is required for KV-cache recovery and compaction.
- **Non-zero budget:** `max_tokens_per_minute` cannot be `0`.
- **Paging space:** if `stateful_paging` is `true`, `max_kv_cache_mb` cannot be `0`.

---

## Security Status Rules

The `ore ls --agents` dashboard labels each agent from its manifest:

| Status | Condition |
|---|---|
| **UNSAFE** | `can_execute_shell = true` |
| **AIR-GAPPED** | No shell access and `network_enabled = false` |
| **SECURED** | No shell access and `network_enabled = true` |

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

### Coding Agent

```toml
app_id = "code_reviewer"
description = "Reviews code, including SQL and shell snippets"
version = "1.0.0"

[resources]
allowed_models = ["qwen2.5:0.5b"]
max_tokens_per_minute = 10000
gpu_priority = "normal"

[execution]
can_execute_wasm = true
is_coding_agent = true                # `DROP TABLE` / `eval(` in prompts won't be blocked
allowed_tools = ["linter"]
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
json_history = true
stateful_paging = false

[ipc]
allowed_agent_targets = ["terminal_user"]
allowed_semantic_pipes = ["rust_docs"]
semantic_persistence = true
allow_time_decay = false
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

[network]
network_enabled = true
allow_localhost_access = false

[[network.rules]]
domain = "arxiv.org"
allowed_methods = ["GET"]
allowed_paths = ["*"]

[[network.rules]]
domain = "docs.rs"
allowed_methods = ["GET"]
allowed_paths = ["*"]

[[network.rules]]
domain = "github.com"
allowed_methods = ["GET"]
allowed_paths = ["*"]

[execution]
can_execute_shell = false
can_execute_wasm = true
allowed_tools = ["file_search", "web_fetch"]

[ipc]
allowed_agent_targets = ["writer_agent"]
allowed_semantic_pipes = ["research_papers", "rust_docs"]
semantic_persistence = true
allow_time_decay = false
```

---

**Next:** [Security Model →](./security-model.md)
