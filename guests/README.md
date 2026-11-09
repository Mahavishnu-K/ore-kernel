# Ore Guest Runtimes (`guests/`)

This directory contains the source code, native C extensions, and build pipelines for the two WebAssembly runtimes the Ore kernel runs scripts in: **`system-py.wasm`** and **`system-js.wasm`**.

These runtimes are the execution engines behind **Inception Mode** (AI agents sending Python or JavaScript to `POST /execute` with a `language` and a `script`) and behind Python/JS **WASM Cartridges** built with `ore mktool`.

## Why this folder exists

AI agents write code on the fly, and Ore has to run it without trusting it. Ore does that inside its WASM sandbox, so it needs Python and JavaScript interpreters compiled to WebAssembly. Stock interpreter builds can't do the job, for four reasons:

1. **They can't talk to the kernel.** A WASM guest can only call the host functions it imports. These runtimes compile in a small C bridge that imports the kernel's `ore::fetch` / `ore::fetch_async`, so code in the sandbox can reach the kernel's firewalled network directly. A downloaded `python.wasm` or QuickJS build has no such import.
2. **They must be self-contained.** The kernel shouldn't depend on a Python or Node installation on the host, and a sandboxed script can't read the host's files. `system-py.wasm` carries its whole standard library inside the binary; `system-js.wasm` gets Node APIs from Ore's own polyfills, mounted read-only.
3. **They must fit the sandbox's limits.** Every execution has a CPU fuel budget (5 billion instructions). Ore's builds are tuned for it. For example, Python's bytecode is prebuilt in a way that avoids recompiling the standard library on every run, which would use up the whole budget on its own.
4. **They must be reproducible and auditable.** The runtimes execute every agent script, so they're built from pinned upstream sources (CPython 3.12.3, QuickJS-NG v0.18.0, wasi-sdk 22, wasi-vfs 0.5.4) with scripts anyone can read and rerun, not shipped as opaque binaries.

| Runtime | Engine | Built with | Size |
|---|---|---|---|
| `system-py.wasm` | CPython 3.12.3 for `wasm32-wasi`, standard library packed inside | Docker (wasi-sdk 22) | ~27 MB |
| `system-js.wasm` | QuickJS-NG v0.18.0 for `wasm32-wasi` | Zig | ~5 MB |

The kernel loads them from `<ore_dir>/runtimes/` on each execution and caches the compiled machine code next to them as `.cwasm`. A newer `.wasm` invalidates that cache automatically.

---

## Architecture: the native bridge

```text
┌────────────────────────────────────────────────────────────┐
│              AI AGENT SCRIPT (inside the sandbox)          │
│    Python: import _ore                                     │
│    JavaScript: globalThis.__ore__                          │
└─────────────────────────────┬──────────────────────────────┘
                              │ direct call, no file polling
                              ▼
┌────────────────────────────────────────────────────────────┐
│               NATIVE C EXTENSION (GUEST)                   │
│    Python: python/_ore_module.c   (built-in module _ore)   │
│    QuickJS: quickjs/quickjs-ore.c (globalThis.__ore__)     │
└─────────────────────────────┬──────────────────────────────┘
                              │ WASM import: ore::fetch / ore::fetch_async
                              ▼
┌────────────────────────────────────────────────────────────┐
│                  ORE KERNEL (RUST HOST)                    │
│    ore-core/src/sandbox/shims.rs                           │
│    egress firewall ([[network.rules]]) → HTTP → /ore_tmp   │
└────────────────────────────────────────────────────────────┘
```

Both host functions take `(method, url, headers, body, filename)` as pointer/length pairs into guest memory. `headers` is a JSON object (or empty), and the response body is streamed to `/ore_tmp/<filename>`. Every call is checked against the agent's manifest egress rules first.

| Return value | Meaning |
|---|---|
| `200`–`599` | HTTP status of the response |
| `-1` | Blocked: domain, path, scheme or localhost not allowed, unreadable arguments, malformed headers JSON, or path traversal in `filename` |
| `-2` | Blocked: HTTP method not allowed for this domain |
| `-3` | HTTP failure (client error, connection, interrupted stream) |
| `-4` | Writing the response file failed |

| Runtime | Exposed as | Source |
|---|---|---|
| Python | built-in module `_ore`: `_ore.fetch(...)`, `_ore.fetch_async(...)` | `python/_ore_module.c` |
| JavaScript | global `__ore__`: `__ore__.fetch(...)`, `__ore__.fetchAsync(...)` | `quickjs/quickjs-ore.c` |

**Current status:** the bridge is compiled into both runtimes, but Ore's own shims don't use it yet. The kernel's Python preamble (`ore-server/shims/python/bootstrap.py`) and the JS `http.js` polyfill still route `requests`/`httpx`/`urllib`/`fetch` through the file-based network portal (`/ore_tmp/.ore_network/`). Python and TypeScript SDKs that wrap the bridge (`ore-sdk`, `@ore/sdk`) are on the [ROADMAP](../ROADMAP.md).

In JavaScript, arguments are passed as strings (UTF-8), so `__ore__` handles text and JSON bodies. Truly binary bodies would need `ArrayBuffer` support in `quickjs-ore.c`.

---

## Requirements

| | Python runtime | JS runtime |
|---|---|---|
| Docker (Docker Desktop on Windows, **running**) | ✓ | |
| Zig 0.17 (on `PATH`, `ORE_ZIG`, or `~/.ore/toolchains/zig-0.17.0/zig`) | | ✓ |
| Git and a bash shell | ✓ | ✓ |

**Windows:** run the scripts from **Git Bash**. Typing `bash` in PowerShell may start WSL instead, which has a different `HOME`, no Windows `zig`, and copies the output into the WSL filesystem. `guests/.gitattributes` keeps `*.sh` files at LF line endings; bash fails on CRLF.

---

## 1. Python runtime (`system-py.wasm`)

```bash
cd guests/python
./build.sh                      # add  2>&1 | tee build.log  to keep a log
```

The first build takes a while because CPython is compiled twice. Docker caches every step, so changing only the last steps rebuilds in seconds.

### What `Dockerfile.build` does

1. Installs wasi-sdk 22, the `wasi-vfs` CLI and `libwasi_vfs.a` (v0.5.4).
2. Cross-compiles zlib 1.3.1 for WASI, so `zlib`, `gzip` and compressed `zipfile` work in the sandbox.
3. Builds a native host Python 3.12.3, which a cross build needs to generate bytecode.
4. Configures CPython for `wasm32-unknown-wasi` with `Tools/wasm/config.site-wasm32-wasi`, links `libwasi_vfs.a`, and compiles `_ore` in statically through `Modules/Setup.local` in the build directory.
5. Builds `python.wasm` and installs the standard library under `/opt/python-wasi`.
6. Prunes what the sandbox can't use: `idlelib`, `tkinter`, `turtledemo`, `ensurepip`, `lib2to3`, `config-3.12-wasm32-wasi/` (a 35 MB `libpython3.12.a`), and `.opt-1`/`.opt-2` bytecode.
7. Recompiles the standard library as unchecked-hash `.pyc` files.
8. Strips DWARF debug info (about 19 MB). The name section stays, so traps still show function names.
9. Packs the standard library into the binary with `wasi-vfs pack`, at `/opt/python-wasi/lib/python3.12`.

### Why some steps look the way they do

| Setting | Reason |
|---|---|
| `--host=wasm32-unknown-wasi` | CPython 3.12's configure only recognizes `*-*-wasi`. `wasm32-wasip1` is for 3.13 and later |
| `PKG_CONFIG_PATH=""` and `PKG_CONFIG_LIBDIR=<wasi sysroot>:<wasi zlib>` | Stops host libraries (the host `zlib.pc`) from leaking into the WASI build, as CPython's own `Tools/wasm/wasi-env` does |
| `ac_cv_func_dlopen=no` | wasi-sdk 22's libc has a `dlopen` symbol, which makes 3.12 build `xxlimited` as a `.so`. WASI can't build or load shared modules, so `make install` would fail |
| `--with-ensurepip=no` on the host build | The host Python only generates bytecode; pip isn't needed |
| Unchecked-hash `.pyc` (PEP 552) | `wasi-vfs` reports every packed file's modification time as 0, so timestamp-checked `.pyc` files all look stale. Python would then recompile the stdlib on every run, which exhausts the kernel's 5-billion-instruction fuel limit. With unchecked-hash `.pyc` files the bootstrap imports take about 700 million instructions. The packed stdlib can't change, so skipping the source check is safe |
| `wasi-vfs pack` with an `ore` import | `pack` runs the module once during packing (it wraps Wizer). Wizer stubs every unknown function import, so `ore.fetch` doesn't stop it. The packing code never calls it, and the real import stays for the kernel to provide |

### Limits

No threads, no sockets, and no compiled extension modules: only pure-Python packages work. For `dependencies` in `/execute`, the kernel installs packages with pip on the host and mounts them read-only.

---

## 2. JavaScript runtime (`system-js.wasm`)

```bash
cd guests/quickjs
./build.sh
```

The QuickJS build uses Zig as a C compiler targeting WebAssembly, so it doesn't need Docker.

### What `build.sh` does

1. Clones QuickJS-NG **v0.18.0** into `quickjs-src/` (pinned; reused on later runs, and gitignored).
2. Resets `qjs.c` and the shipped `gen/repl.c` / `gen/standalone.c`, then copies in `quickjs-ore.c` / `.h`.
3. Patches `qjs.c` in one place: `js_init_ore_bridge(ctx)` is inserted in `JS_NewCustomContext`, after its NULL check, so every context gets `globalThis.__ore__`. The build stops if the patch doesn't apply exactly once, so an upstream change can't produce a broken `qjs`.
4. Compiles with `zig cc -target wasm32-wasi -O3` with WASI signal and process-clock emulation and a 2 MiB stack, as QuickJS-NG's own WASI build does.

Apart from that patch, the runtime is stock QuickJS-NG with standard ES-module resolution.

### Node.js polyfills (`runtimes/modules/`)

QuickJS-NG has no Node APIs. Its built-in modules are `qjs:std`, `qjs:os` and `qjs:bjson`. Node's `fs`, `path`, `Buffer`, `events`, `http`, `crypto`, `process` and the rest come from Ore's polyfills in `<ore_dir>/runtimes/modules/`, which the kernel mounts at `/modules`:

- The kernel rewrites a script's `import fs from 'fs'` (and `node:fs`, `require('fs')`) to `/modules/fs.js`, and prepends `ore-server/shims/javascript/commonjs.js`, which sets up `process`, `Buffer`, `fetch`, `require()` and other globals.
- The polyfills import QuickJS-NG's modules by their real names (`qjs:std`, `qjs:os`) and each other by relative path. There are no bare specifiers.
- QuickJS-NG has timers only on `qjs:os`, so `_host_timers.js` installs Node-compatible global `setTimeout` / `setInterval` / `clearTimeout` / `clearInterval`. Every polyfill that uses timers imports it. QuickJS can't make a timer stop holding the script open, so `unref()` only records the flag.
- `crypto.createHash` and similar run in the kernel through the crypto portal (`/ore_tmp/.ore_crypto/`); QuickJS-NG has no crypto.

**Release them together.** `ore pull system-js` installs `system-js.wasm` and the `modules/` folder from the same GitHub release. The polyfills need QuickJS-NG (`qjs:std`), and the runtime needs these polyfills, so always publish both.

---

## Output and verification

Both scripts copy their output to `<repo>/runtimes/` and, if it exists, to `~/.ore/runtimes/`. No kernel restart is needed. On the next execution the kernel logs `.wasm is newer than .cwasm. Invalidating AOT Cache...` and recompiles.

With the kernel running:

```bash
cd examples/wasm_agents
python run_inception.py   # Python stdlib, zlib, _ore, pip dependencies; JS Node modules; npm (pdf-lib)
python run_network.py     # Python requests/httpx/urllib/asyncio; JS fetch, axios, node-fetch, https streams
```

A quick check of the bridge from Python:

```python
import _ore, zlib, json
print(_ore.fetch, _ore.fetch_async, zlib.compress(b"ore"))
```

---

## Modifying the linker bridge

If you change the `ore::fetch` or `ore::fetch_async` signatures in `ore-core/src/sandbox/shims.rs`, you **must** update the matching declarations in both C files:

1. `guests/python/_ore_module.c`
2. `guests/quickjs/quickjs-ore.c`

Then rebuild and restart the kernel (`ore-server`), rebuild both runtimes with their `build.sh`, and publish the new runtimes (with `modules/` for JS).
