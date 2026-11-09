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
- **Source:** [`ore-server/src/execution/script.rs`](../../ore-server/src/execution/script.rs) · [`guests/`](../../guests/README.md)
- Allows agents to dynamically materialize and execute scripts in Python or JavaScript/TypeScript.
- Manifest check: Language in `allowed_language_runtimes` (or `"*"`).
- Automatically provisions interpreters from `runtimes/` (`system-py.wasm`, `system-js.wasm`).
- **Pre-compiled Guest Runtimes with Native C Bridges**:
  - **Python (`system-py.wasm`)**: CPython 3.12.3 built with `wasi-sdk 22`, cross-compiled zlib 1.3.1, and stdlib packed via `wasi-vfs` with PEP 552 unchecked-hash `.pyc` files to prevent stdlib re-compilation and preserve CPU fuel (~700M instructions vs 5B fuel limit). Embeds native C extension `_ore_module.c` exposing `_ore.fetch()` and `_ore.fetch_async()`.
  - **JavaScript (`system-js.wasm`)**: QuickJS-NG v0.18.0 built with Zig 0.17 with Node.js polyfills mounted to `/modules`. Embeds native C bridge `quickjs-ore.c` / `quickjs-ore.h` injecting `globalThis.__ore__`.
- **Dynamic Dependency Management & JIT Caching**:
  - **Python**: Detects `requirements.txt` or explicit dependencies, stages modules, mounts them dynamically to VFS, and sets `PYTHONPATH=/packages:/app:/workspace/{hash}` with `PYTHONUNBUFFERED=1`.
  - **Pure TypeScript Fast-Path**: Pure TypeScript scripts (`lang == "ts"` or `"typescript"` with no npm packages) use a dedicated staging directory (`cache/npm/_transpile_ts/`) directly bundled with `esbuild`, bypassing npm entirely.
  - **Smart NPM Dependency Caching**: Package requirement lists are sorted and hashed via SHA-256 (`KernelCrypto::sha256`), cached under `cache/npm/<req_hash>/`. Cache hits instantly skip `npm install` and proceed directly to `esbuild` bundling with `--external:/modules/*`.
  - **Top-Level Await & Error Boundary**: JavaScript/TypeScript inception code is wrapped via `wrap_tla_with_error_boundary`, which isolates static ES module imports and encloses execution in a Promise block to enable native top-level `await` and safely route unhandled rejections to STDERR.
  - **Robust Diagnostics**: If launching `npm` or `esbuild` fails or returns a non-zero exit code, clear `KERNEL ERROR: Failed to launch...` messages are routed back to the agent without crashing the kernel.

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
├── shims.rs             # Host syscalls (ore::fetch, ore::fetch_async), dynamic shadowing (sock_accept) & OS table
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

The sandbox has **zero raw TCP/UDP socket access**. Outbound HTTP traffic is strictly checked against manifest `[[network.rules]]` through two complementary channels:

1. **Direct WASM Host Syscalls (`ore::fetch`, `ore::fetch_async`)**: Instant trap-based execution without file-polling overhead, used by native C bridges (`_ore`, `__ore__`) and compiled WASM tools.
2. **Asynchronous File-based Portal (`.ore_network/`)**: High-level drop-in compatibility for un-ported Python/JS libraries (`requests`, `httpx`, `fetch`, `axios`).

```text
WASM Guest (Direct Syscall or Language Shim)
       │
       ├──────────────────────────────────────────────────────┐
       │ (1) Direct Host Syscall: ore::fetch / fetch_async   │ (2) Language Shim File Portal
       ▼                                                      ▼
┌──────────────────────────────────────────────┐       ┌──────────────────────────────────────────────┐
│ Host Linker Shims (ore-core/src/sandbox/     │       │ Background Network Watcher (portals/         │
│ shims.rs)                                    │       │ network.rs)                                  │
│ · Bounds-checked read_mem_bytes/string       │       │ · Watch .ore_network/req_<id>.json           │
│ · Validate headers JSON: parse_headers_json  │       │ · Parse method, url, headers, body           │
└──────────────────────┬───────────────────────┘       └──────────────────────┬───────────────────────┘
                       │                                                      │
                       └──────────────────────┬───────────────────────────────┘
                                              ▼
                               ┌──────────────────────────────────────────────┐
                               │ check_egress() (sandbox/net/fetch.rs)        │
                               │ · Protocol: http / https only                │
                               │ · Localhost block (127.0.0.1, 169.254.x, etc)│
                               │ · Domain match (manifest rule or "*")        │
                               │ · Method match (GET, POST, etc.)             │
                               │ · Path match (glob / prefix / exact)         │
                               └──────────────────────┬───────────────────────┘
                                                      │
                                                      ▼
                               ┌──────────────────────────────────────────────┐
                               │ HTTP Execution (reqwest, 60s timeout)        │
                               │ · Redirects re-checked via check_egress()    │
                               │ · Stream response body directly to /ore_tmp  │
                               │ · Write res.bin + res.meta (or .done)        │
                               └──────────────────────────────────────────────┘
```

### Direct Host Syscalls (`ore-core/src/sandbox/shims.rs`)

Compiled tools and native C guest extensions import the host functions from the `ore` module:

```c
// 10-parameter ABI passing pointers and lengths in guest linear memory
__attribute__((import_module("ore"), import_name("fetch")))
extern int ore_fetch(
    const char* method_ptr, int method_len,
    const char* url_ptr, int url_len,
    const char* headers_ptr, int headers_len,
    const char* body_ptr, int body_len,
    const char* filename_ptr, int filename_len
);

__attribute__((import_module("ore"), import_name("fetch_async")))
extern int ore_fetch_async(
    const char* method_ptr, int method_len,
    const char* url_ptr, int url_len,
    const char* headers_ptr, int headers_len,
    const char* body_ptr, int body_len,
    const char* filename_ptr, int filename_len
);
```

- **`ore_fetch` (Synchronous)**: Blocks the guest thread while the host executes the request and streams the response body to `/ore_tmp/<filename>`. Returns the HTTP status code (`200`..`599`) or negative error code on firewall denial.
- **`ore_fetch_async` (Asynchronous)**: Immediately queues the request on a background OS thread and returns `0`. The background thread streams metadata to `/ore_tmp/<filename>.meta` upon receipt of HTTP response headers and writes `/ore_tmp/<filename>.done` when transmission completes (or writes a 502 sentinel on failure).
- **Linear Memory Safety**: All pointer/length pairs are validated via `read_mem_bytes` and `read_mem_string`. If `ptr + len` exceeds the guest's allocated WebAssembly memory, or if the headers JSON is malformed, the call fails immediately with `-1`.

| Return Code | Meaning |
|---|---|
| `200`–`599` | Upstream HTTP response status |
| `0` | Asynchronous fetch successfully dispatched |
| `-1` | Firewall blocked (disallowed domain, path, localhost address, traversal in `filename`, or malformed headers) |
| `-2` | Disallowed HTTP method for this domain |
| `-3` | HTTP transport failure (connection error, timeout, aborted stream) |
| `-4` | Failed to write response file to VFS |

### Key Security Guarantees
- **No Direct Socket Access**: Guest tools cannot establish raw socket connections, scan ports, or bypass firewalls.
- **Path Traversal Protection**: Filenames written by network downloads are sanitized against `..`, `/`, and `\` paths.
- **Zero-RAM Response Streaming**: Incoming HTTP bodies stream directly to ephemeral files in `/ore_tmp`, preventing agents from crashing host memory with large payloads.
- **Ephemeral Auto-Destruction**: The `/ore_tmp` host folder is bound to a Rust `TempDirGuard` and wiped immediately when execution ends.

### Path Rules

`allowed_paths` entries match the URL path (after the URL parser has normalized `..` segments):

| Entry | Matches |
|---|---|
| `"*"` | Every path |
| `"/repos*"` | Paths starting with `/repos` |
| `"/api"` | `/api` and everything below it (`/api/v1/…`), but not `/apiary` |

An empty list allows nothing. The policy is covered by unit tests in `sandbox/net/fetch.rs`, including a live redirect to `169.254.169.254` that must be blocked.

### Current Limits
- Private LAN ranges (`10.x`, `172.16–31.x`, `192.168.x`) aren't treated as local, so a `"*"` domain rule can reach them.
- Hosts are checked by name, not by resolved IP (no DNS-rebinding protection).

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
