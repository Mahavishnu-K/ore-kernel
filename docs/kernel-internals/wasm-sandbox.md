# Zero-Trust WASM Sandbox & Execution Engine

> Modular execution (Tool, Script, Shell), AOT Zero-RAM caching (`.cwasm`), Layer 7 Network Portal, and Capability-Based VFS.

**Source:** [`ore-core/src/sandbox/`](../../ore-core/src/sandbox/) · [`ore-server/src/execution/`](../../ore-server/src/execution/)

---

## Overview

When an AI agent executes code, scrapes the web, or mutates data, giving it raw host access is catastrophic (prompt injection leading to arbitrary command execution or secret exfiltration).

Ore addresses this through a modular execution engine and a Zero-Trust WebAssembly Sandbox powered by `wasmtime` and WASI. Untrusted code runs in a sandboxed virtual machine with deterministic CPU limits, isolated virtual filesystems, and strict Layer 7 network egress controls.

---

## Modular Execution Architecture

Execution requests sent to `POST /execute` are handled by `ore-server/src/execution/`:

```text
POST /execute
      │
      ▼
┌──────────────────────────────────────────────┐
│ execution::execute_request()                 │  ore-server/src/execution/mod.rs
│ 1. Validate Agent Manifest in Registry       │
│ 2. Determine ExecutionMode                   │
└──────────────────────┬───────────────────────┘
                       │
       ┌───────────────┼───────────────┐
       ▼               ▼               ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│ Tool Mode   │ │ Script Mode │ │ Shell Mode  │
│ (tool.rs)   │ │ (script.rs) │ │ (shell.rs)  │
└──────┬──────┘ └──────┬──────┘ └──────┬──────┘
       │               │               │
       │    Prepare    │    Prepare    │ Direct Host Execution
       │    Cartridge  │    Workspace  │ (can_execute_shell = true)
       │               │    & Shims    │
       └───────┬───────┴───────┘
               ▼
┌──────────────────────────────────────────────┐
│ WasmSandbox::execute(params)                 │  ore-core/src/sandbox/mod.rs
│ · Capability-based VFS (/workspace, /ore_tmp)│
│ · AOT Zero-RAM Caching (.cwasm mmap)         │
│ · CPU Fuel Limits (max_cpu_instructions)     │
│ · Network Portal (Layer 7 Egress Proxy)      │
│ · Crypto Portal (Host Hardware Pipes)        │
└──────────────────────────────────────────────┘
```

### 1. Fixed Tool Mode ("Console Cartridges")
- **Source:** [`ore-server/src/execution/tool.rs`](../../ore-server/src/execution/tool.rs)
- Executes pre-compiled `.wasm` binaries stored in `tools/`.
- Manifest check: `manifest.execution.can_execute_wasm = true` and tool in `allowed_tools` (or `"*"`).
- Arguments passed via WASI command-line args; complex inputs piped via STDIN (`input_data`).

### 2. Autonomous Script Mode ("Inception Mode")
- **Source:** [`ore-server/src/execution/script.rs`](../../ore-server/src/execution/script.rs)
- Allows agents to dynamically materialize and execute scripts in Python or JavaScript/TypeScript.
- Manifest check: Language in `allowed_language_runtimes` (or `"*"`).
- Automatically provisions interpreters from `runtimes/` (e.g., `system-py.wasm`, `system-js.wasm`).
- **Dynamic Dependency Management**:
  - **Python**: Detects `requirements.txt` or explicit dependencies, stages modules, mounts them dynamically to VFS, and sets `PYTHONPATH=/packages:/app:/workspace/{hash}` with `PYTHONUNBUFFERED=1`.
  - **JavaScript/Node.js**: Automatically mounts polyfilled modules into `/modules` (Read-Only) and injects CommonJS runtime shims.
  - **Top-Level Await & Error Boundary**: JavaScript/TypeScript inception code is wrapped via `wrap_tla_with_error_boundary`, which isolates static ES module imports and encloses execution in a Promise block to enable native top-level `await` and safely route unhandled rejections to STDERR.

### 3. Raw Host Shell Bypass (UNSAFE)
- **Source:** [`ore-server/src/execution/shell.rs`](../../ore-server/src/execution/shell.rs)
- Bypasses the sandbox completely to run host commands (e.g., `npm run dev`, `git status`).
- Manifest check: Structurally rejected unless `manifest.execution.can_execute_shell = true`.
- Agents with this permission are permanently flagged as **UNSAFE** in security dashboards.

---

## Core Sandbox Subsystem (`ore-core/src/sandbox/`)

The kernel sandbox is architected into modular components:

```text
ore-core/src/sandbox/
├── mod.rs               # Hypervisor facade (WasmSandbox struct & linear execution pipeline)
├── params.rs            # Data contracts (ExecuteParams, OreResponseMeta)
├── state.rs             # OreSandboxState + RAII guards (TempDirGuard, ThreadJoinGuard)
├── cache.rs             # Ahead-Of-Time (.cwasm) compiler, timestamp validator & mmap loader
├── vfs.rs               # WASI VFS configuration, UNC sanitization & capability mounts
├── shims.rs             # Host syscalls (ore::fetch), dynamic shadowing (sock_accept) & OS table
│                        # (the ore-ld linker in ore-core/src/linker/ is registered here too)
├── portals/             # Concurrent VFS background daemons
│   ├── mod.rs           # Portal daemon coordinator (start_all)
│   ├── crypto.rs        # Dedicated .ore_crypto portal (atomic req.bin / res.bin loop)
│   └── network.rs       # Concurrent .ore_network watcher (req_*.json / res_*.bin)
└── net/                 # Layer 7 Egress Network Engine
    ├── mod.rs           # Network exports
    └── fetch.rs         # execute_ore_fetch (domain/method whitelist, streaming, cookies)
```

- **RAII Resource Management**: `TempDirGuard` guarantees automatic deletion of ephemeral directories (`~/.ore/tmp/<exec_id>`) upon task completion, traps, or panics. `ThreadJoinGuard` ensures background portal threads gracefully terminate and join.
- **Dynamic Shadowing**: Legacy socket calls expected by interpreters (such as QuickJS `sock_accept`) are shadowed via `linker.allow_shadowing(true)` and cleanly stubbed to `ENOSYS`.
- **Trap Defense**: `linker.define_unknown_imports_as_traps(&module)` ensures unexpected third-party imports halt safely instead of failing VM instantiation.

---

## Runtime Shims & Polyfills

To allow modern Python and JavaScript packages to run inside pure WASI without native sockets:

### JavaScript CommonJS Shim (`shims/javascript/commonjs.js`)
- Injects standard Node.js globals: `process`, `Buffer`, `fetch`, `Headers`, `Request`, `Response`, `TextEncoder`, `TextDecoder`, `URL`, `setImmediate`.
- Implements a universal CommonJS `require()` bridge mapping module imports to `/modules/<name>.js`.
- Provides an empty proxy fallback for unsupported optional built-ins (`cluster`, `tty`) so imports never freeze execution.

### Python WASI Bootstrap Shim (`shims/python/bootstrap.py`)
- **WASI Asyncio Event Loop**: Implements `WasiEventLoop` and `WasiSelector` bypassing missing `socketpair` calls, utilizing non-blocking `time.sleep` poll hooks that suspend the VM without burning CPU instructions.
- **Transparent `requests` & `httpx` Polyfills**: Shims `requests.get()`, `requests.post()`, `requests.Session()`, `httpx.get()`, `httpx.post()`, and `httpx.Client()` directly into the Ore Network Portal.
- **HTTP Metadata & Headers**: Implements `CaseInsensitiveDict` for headers, cookie jar persistence, and `ORE_Response` streaming.

---

## AOT (Ahead-Of-Time) Zero-RAM Caching (`.cwasm`)

Standard WASM JIT compilation adds ~300ms to ~600ms latency on cold starts. Ore eliminates this with native Ahead-Of-Time serialization:

```text
First Execution (Cold Start)
WASM Binary ──▶ Wasmtime JIT ──▶ Compiled Native Code ──▶ Save .cwasm to disk
                                                                   │
Subsequent Executions (Cache Hit)                                  ▼
.cwasm File ──▶ OS mmap (Module::deserialize_file) ──▶ Instant Boot (~10ms)
```

1. **Zero-RAM Page Cache Mapping**: `Module::deserialize_file` maps the `.cwasm` file using the OS `mmap` syscall. Memory pages are lazily loaded directly into the CPU instruction cache from the OS Page Cache with **0 MB allocated RAM bloat**.
2. **Filesystem Timestamp Invalidation**: The kernel compares `std::fs::metadata` modification timestamps (`mtime`) between `.wasm` and `.cwasm`. If the `.wasm` file is modified, the stale `.cwasm` is invalidated and recompiled automatically. No in-memory state or cache maps are required.

---

## Layer 7 Network Portal & Egress Firewall

The sandbox has **zero raw TCP/UDP socket access**. All network I/O is routed through kernel-controlled portals:

```text
WASM Guest (any language via ore.fetch, or Python / JS via the shims)
       │
       │ Writes request to .ore_network/req_<id>.json (in its /ore_tmp)
       ▼
┌──────────────────────────────────────────────┐
│ Background Network Watcher Thread (Host)     │  sandbox/portals/network.rs
│ 1. Parse URL & target host                   │
│ 2. Localhost check (allow_localhost_access)  │  sandbox/net/fetch.rs
│ 3. Check [[network.rules]] in Manifest       │
│    - Host must equal `domain` (or "*")       │
│    - Method must be in `allowed_methods`     │
│ 4. Execute reqwest HTTP call (60 s timeout)  │
│ 5. Stream response body to disk              │
│ 6. Write res_<id>.bin + res_<id>.meta        │
└──────────────────────┬───────────────────────┘
                       │
                       ▼
       Guest reads the response files back
```

### Key Security Guarantees
- **No Direct Socket Access**: Guest tools cannot establish raw socket connections, scan ports, or bypass firewalls.
- **Path Traversal Protection**: Filenames written by network downloads are sanitized against `..`, `/`, and `\` paths.
- **Zero-RAM Response Streaming**: Incoming HTTP bodies stream directly to ephemeral files in `/ore_tmp`, preventing agents from crashing host memory with large payloads.
- **Ephemeral Auto-Destruction**: The `/ore_tmp` host folder is bound to a Rust `TempDirGuard` and wiped immediately when execution ends.

### Current Limits
- `[[network.rules]].allowed_paths` is parsed but **not enforced**. Only the host and method are checked.
- The HTTP client follows redirects (up to 10) without re-checking them against the rules.
- Only `localhost`, `127.0.0.1`, `0.0.0.0`, and `[::1]` count as local. Private ranges such as `10.x` and `192.168.x` aren't blocked.

See [Security Model → Known Limitations](../security-model.md#known-limitations).

---

## Virtual File System (VFS)

The sandbox isolates file I/O using capability-based directories (`cap-std`):

```text
/ (Guest Root)
├── workspace/
│   ├── [Read-Only Mounts]   <── DirPerms::READ, FilePerms::READ
│   └── [Read-Write Mounts]  <── DirPerms::all(), FilePerms::all()
├── ore_tmp/                 <── Ephemeral Network & Crypto Cache
└── modules/                 <── Read-Only JS Runtime Polyfills
```

- **Read-Only Paths (`allowed_read_paths`)**: Strictly stripped of write capabilities at the WASI boundary. Even if the host OS allows writing, the sandbox traps any modification attempts.
- **Read-Write Paths (`allowed_write_paths`)**: Mapped strictly within manifest bounds.
- **VFS Crypto Portal (`/.ore_crypto/`)**: Safe atomic host worker pipes for low-latency hardware crypto operations without breaking WASI compute boundaries.

---

## Deterministic CPU Fuel Limits

- Fuel metering is enabled on the engine (`config.consume_fuel(true)`) and injected into each store via `store.set_fuel(fuel_limit)`.
- Configured by manifest: `[execution] max_cpu_instructions` (default: **5,000,000,000 instructions**, roughly 2-3 seconds of compute).
- If an agent script enters an infinite loop or runaway computation, execution halts with `Trap::OutOfFuel`, protecting host resources.
- There is **no memory limit** yet (no `StoreLimits`): a guest can grow its linear memory up to the wasm32 maximum of 4 GiB.

---

## How a Run Ends (Result Reporting)

`WasmSandbox::format_output` decides the outcome from wasmtime's **typed error**, never from the error message. The message includes the guest's wasm backtrace, so matching on text (for example Zig's `start.startWasi` frame) could mistake a crash for a clean exit.

The response always contains the guest's stdout, followed by `--- STDERR ---` and stderr if stderr isn't empty. stdout and stderr are each captured into a 10 MiB in-memory pipe. Then:

| Outcome | Detected by | Appended to the output |
|---|---|---|
| Entry point returned | `Ok(())` | nothing |
| `proc_exit(0)` (e.g. returning from `main`) | `wasmtime_wasi::I32Exit(0)` | nothing |
| `proc_exit(n)`, `n ≠ 0` (Go panic → 2, uncaught Python exception → 1) | `wasmtime_wasi::I32Exit(n)` | `KERNEL ERROR: Guest exited with status n` |
| Fuel ran out | `wasmtime::Trap::OutOfFuel` | `KERNEL ERROR: Sandbox Trap: CPU Fuel Exhausted (Runaway AI or Infinite Loop Detected)` |
| Any other trap (Rust panic, C `abort`, out-of-bounds access, null or stale `call_indirect`, stack overflow) | anything else | `KERNEL ERROR: <trap>` |

Every failure ends with exactly one `KERNEL ERROR:` line, so an agent can detect failure the same way for every language. The kernel logs `[SANDBOX] Program exited …` for exits and `[SANDBOX TRAP] …` for traps. These cases are covered by unit tests in `ore-core/src/sandbox/mod.rs`.

---

**← Back to:** [Kernel Internals Index](./README.md)
