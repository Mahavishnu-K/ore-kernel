# Ore Guest Runtimes (`guests/`)

This directory contains the source code, native C-extensions, and build pipelines used to forge the base WebAssembly runtimes for the Ore Kernel: **`system-py.wasm`** and **`system-js.wasm`**.

These runtimes are the execution engines that power **Inception Mode** (autonomous AI scripts) and **WASM Cartridges** built via `ore mktool`.

## Architecture: The Native Linker Bridge

To provide AI agents and developers with blazing-fast network access without the overhead of Virtual File System (VFS) polling, these runtimes embed a low-level C-bridge directly into their core interpreters.

```text
┌────────────────────────────────────────────────────────────┐
│                  DEVELOPER / AI AGENT CODE                 │
│    Python: `from ore import http`                          │
│    TypeScript: `import { http } from '@ore/sdk'`           │
└─────────────────────────────┬──────────────────────────────┘
                              │ (SDK wraps the C-bridge)
                              ▼
┌────────────────────────────────────────────────────────────┐
│                 NATIVE C-EXTENSION (GUEST)                 │
│    Python: `_ore_module.c` (import _ore)                   │
│    QuickJS: `quickjs-ore.c` (globalThis.__ore__)           │
└─────────────────────────────┬──────────────────────────────┘
                              │ (Direct CPU Instruction Trap)
                              ▼
┌────────────────────────────────────────────────────────────┐
│                 ORE KERNEL (RUST HOST)                     │
│    `ore-core/src/sandbox/shims.rs` (`ore::fetch`)          │
└────────────────────────────────────────────────────────────┘
```

By compiling the C-bridge directly into the language runtimes, network calls achieve **sub-50 microsecond** latency, instantly triggering the host's Layer 7 Egress Firewall and streaming responses securely to the SSD.

---

## 1. Python Runtime (`system-py.wasm`)

A "batteries-included" CPython 3.12 interpreter compiled for `wasm32-wasip1`.

### Features
* Embeds `_ore_module.c` as a static, built-in module (`import _ore`).
* Uses `wasi-vfs` to pack the entire Python Standard Library (`os`, `json`, `math`, `encodings`, etc.) directly into the data section of the WebAssembly binary.
* Results in a highly portable, self-contained ~25.1 MB runtime.

### How to Build
The Python build requires Docker to ensure a reproducible cross-compilation environment using `wasi-sdk`.

```bash
cd python
./build.sh
```
* **Windows Users**: You must run `./build.sh` from **Git Bash** or WSL. Ensure your Git configuration does not convert line endings to CRLF (the script must use `LF`).
* **Output**: The script will automatically copy `system-py.wasm` to `~/.ore/runtimes/`.

---

## 2. JavaScript / TypeScript Runtime (`system-js.wasm`)

A lightning-fast, ultra-lightweight JavaScript engine based on **QuickJS-NG**.

### Features
* Boots in ~50 microseconds with ~0MB idle RAM.
* Patches `qjs.c` at compile-time to inject `quickjs-ore.c`, exposing `globalThis.__ore__` to the JavaScript context.
* Designed to work perfectly with Ore's `/modules` Node.js polyfill mounts.

### How to Build
The QuickJS build uses **Zig** as a drop-in C compiler targeting WebAssembly. It does not require Docker.

```bash
cd quickjs
./build.sh
```
* **Requirements**: You must have Zig installed (or let Ore's `ore mktool` download it to `~/.ore/toolchains/zig-0.17.0/zig`).
* **Windows Users**: You must run `./build.sh` from **Git Bash**. Ensure `build.sh` is saved with **LF** line endings, not CRLF.
* **Output**: The script will automatically copy `system-js.wasm` to `~/.ore/runtimes/`.

---

## Modifying the Linker Bridge

If you update the `ore::fetch` or `ore::fetch_async` host function signatures in the Rust Kernel (`ore-core/src/sandbox/shims.rs`), you **must** update the corresponding signatures in both C files:

1. `guests/python/_ore_module.c`
2. `guests/quickjs/quickjs-ore.c`

After modifying the C files, re-run both `build.sh` scripts to generate the updated `.wasm` runtimes, and restart the `ore-server` daemon.