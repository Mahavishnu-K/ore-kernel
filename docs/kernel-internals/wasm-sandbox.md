# Zero-Trust WASM Sandbox

> The secure execution environment for ORE agents, enabling cross-language tooling and autonomous scripting without compromising the host machine.

## Overview

When an AI agent decides it needs to execute a tool, scrape a web page, or run a Python script, it cannot do so directly on the host machine. Doing so would invite disastrous security breaches (e.g., prompt injection leading to `rm -rf /` or reading SSH keys).

ORE solves this by routing all agent execution requests through a strictly controlled **WebAssembly (WASM) Sandbox**, located at `ore-core/src/sandbox.rs`. Powered by `wasmtime` and the WebAssembly System Interface (`WASI`), the sandbox provides mathematical guarantees of safety.

---

## Sandbox Capabilities & Restrictions

The sandbox employs multiple isolation layers to protect the host:

1. **Deterministic CPU Profiling (Fuel Limit):**
   The sandbox injects a strict `50,000,000` instruction "fuel" limit. If an agent writes a script with an infinite loop or attempts a CPU denial-of-service, the sandbox automatically halts execution with an `Out of Fuel` trap when the limit is reached.
2. **Capability-Based File System (`cap-std`):**
   The sandbox is completely blind to the host's actual file system (e.g., `C:/` or `/home`). See the Virtual File System section below for details on how host directories are safely mapped.
3. **I/O Trapping:**
   Tools cannot hijack the host terminal. `STDOUT` and `STDERR` are captured via in-memory `WritePipes`. Additionally, `STDIN` can be programmatically piped into the sandbox (via the `input_data` payload), allowing agents to pass complex JSON or text to tools. 
4. **Thread Isolation:**
   Sandbox execution is CPU-bound. To prevent starving the ORE kernel's async runtime (`tokio`), sandbox evaluations run inside a dedicated `tokio::task::spawn_blocking` thread.
5. **Network Trapping & Zero-RAM Streaming:**
   WASM socket access is fully gated by the manifest's `[network]` rules, allowing granular control over egress domains, HTTP methods, and localhost isolation. Network requests are trapped via a custom `ore_fetch` host function. To prevent an agent from crashing the host by downloading massive files into RAM, the host streams HTTP responses directly to an ephemeral SSD cache mounted at `/ore_tmp` in the guest. This temporary directory is automatically destroyed when the sandbox exits.

---

## Virtual File System (VFS)

The ORE Sandbox isolates file I/O using a capability-based file system (`cap-std`). The WebAssembly guest executes within a strictly defined virtual hierarchy and cannot traverse (using `../`) into the host's actual disk structure.

The Guest VFS structure looks like this:

```text
/
├── workspace/
│   ├── [Read-Only Paths...]
│   └── [Read/Write Paths...]
└── ore_tmp/
    └── [Ephemeral Network Cache]
```

### 1. The `/workspace` Mount
All host directories specified in the agent's manifest are safely mounted into the `/workspace` guest directory using the folder's basename.

- **Read/Write Paths (`allowed_write_paths`)**: Mounted with `DirPerms::all()` and `FilePerms::all()`. Agents can create, modify, and delete files within these mapped directories.
- **Read-Only Paths (`allowed_read_paths`)**: The kernel explicitly strips write permissions at the WASI OS boundary. They are mounted with `DirPerms::READ` and `FilePerms::READ`. **Even if the host folder is writable, the agent cannot modify it.**

### 2. The `/ore_tmp` Mount (Ephemeral Cache)
When an agent is permitted to make network requests, the kernel creates a unique, UUID-based temporary directory on the host's SSD (e.g. `ore_tmp/<uuid>`). This directory is mounted into the guest as `/ore_tmp`.
- **Zero-RAM Data Streaming**: When the `ore_fetch` HTTP trap downloads a file (like an image or HTML page), it streams the bytes directly to `/ore_tmp/filename`. This mathematically prevents a malicious agent from triggering an Out-of-Memory (OOM) crash by downloading massive files into RAM.
- **Auto-Destruction**: This cache is protected by a Rust `TempDirGuard`. The moment the WASM execution completes (or panics/traps), the `Drop` trait automatically wipes the host directory, ensuring no residual data is left behind.

---

## Execution Modes

ORE supports three distinct execution models. **Only one mode can be invoked per request.**

### 1. Fixed Tool Mode ("Console Cartridges")

This mode allows agents to invoke pre-compiled `.wasm` binaries stored in the `/tools` directory. 

**The Cross-Language Advantage:**
Because tools are compiled to the `wasm32-wasi` target, developers can write ORE tools in **any language** (Rust, Go, C, C++, Zig). This eliminates the dependency bloat of requiring a specific language runtime on the host machine. The agent interacts with the tool via standard POSIX inputs (`args` and `STDIN`), making tool development universally accessible.

**Example Payload:**
```json
{
  "app_id": "cyber_spider",
  "tool_name": "web_scraper",
  "args": ["--url", "https://example.com"],
  "input_data": "{ \"extract\": \"links\" }"
}
```

### 2. Autonomous Scripts ("Inception Mode")

Instead of relying on fixed tools, agents can write and execute dynamic scripts on the fly. ORE achieves this by booting pre-compiled interpreter WASM modules stored in the `/runtimes` directory (e.g., `system-py.wasm` for Python or `system-js.wasm` for JavaScript).

**Example Payload:**
```json
{
  "app_id": "data_analyst",
  "language": "python",
  "script": "import json\nprint(json.dumps({'status': 'success'}))"
}
```

### 3. Raw Host Shell Execution (UNSAFE)

If an agent requires direct interaction with the host system (e.g., running `npm run dev`, `git commit`, or system administration), ORE provides a bypass mode.

**Example Payload:**
```json
{
  "app_id": "terminal_user",
  "shell_command": "git log -n 5"
}
```

> **⚠️ CRITICAL WARNING:** The `shell_command` mode entirely bypasses the WASM sandbox. It invokes the host OS shell (`cmd.exe` on Windows, `sh` on Unix). Agents with this permission enabled are permanently flagged as **UNSAFE** in the ORE security dashboard.

---

## Manifest Enforcement

Before the sandbox compiles or executes any code, the kernel validates the request against the agent's `AppManifest`.

| Feature | Manifest Requirement |
|---|---|
| **WASM Execution** | `[execution] can_execute_wasm = true` |
| **Tool Whitelisting** | `tool_name` must be in `[execution] allowed_tools`. Supports `["*"]` to allow all. |
| **Language Runtimes** | `language` must be in `[execution] allowed_language_runtimes`. Supports `["*"]` to allow all. |
| **Host Shell Access** | `[execution] can_execute_shell = true` |
| **File I/O** | Mounted paths pull from `[file_system] allowed_read_paths` and `allowed_write_paths`. |

If any of these checks fail, the kernel instantly rejects the payload with a `KERNEL ALERT: Permission Denied` response, ensuring the agent remains strictly within its defined boundaries.
