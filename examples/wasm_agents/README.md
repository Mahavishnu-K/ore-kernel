# WASM Agents & Inception Examples (`examples/wasm_agents/`)

This directory contains standalone Python test drivers and agent scripts that interact with the Ore Kernel's WebAssembly sandbox and execution engine over HTTP (`POST /execute`).

## Overview of Scripts

| Script | Purpose | Key Features Covered |
|---|---|---|
| `run_inception.py` | Autonomous Python & JavaScript execution | Precompiled stdlib, zlib compression, native `_ore` host calls, npm dependencies (`pdf-lib`), VFS write to `/workspace` |
| `run_network.py` | Sandboxed HTTP network egress | Python `requests`, `httpx`, `urllib`, `asyncio`; JS `fetch`, `axios`, `node-fetch`, streaming HTTPS |
| `run_network_upgrades.py` | Advanced network egress validation | Request headers, cookie jars, response metadata (`.meta`), status code trapping |
| `run_plugin_lifecycle.py` | Dynamic `ore-ld` plugin lifecycle | `ore_dlopen`, `ore_dlsym`, table nulling on `ore_dlclose`, `ore_dlerror`, slab recycling |
| `run_tla_safeguard.py` | Top-Level Await error boundary | ES-module dynamic imports, unhandled Promise rejections routing to STDERR |
| `run_polyglot_fusion.py` | Cross-language zero-copy memory fusion | Passing and mutating memory buffers between C, Rust, C++, and Zig |

## Prerequisites

1. The Ore Kernel running in a separate terminal:
   ```bash
   cargo run --release -p ore-server
   ```
2. Language runtimes installed:
   ```bash
   ore pull system-py
   ore pull system-js
   ```
   *(Or build them locally using the scripts in `guests/`)*
3. Required manifest: Ensure `manifests/wasm_agent.toml` exists in `<ORE_DIR>/manifests/`.

## Running the Examples

```bash
# Test Inception Mode (Python math/zlib/_ore + JavaScript PDF generation):
python run_inception.py

# Test sandboxed network calls across Python and JavaScript:
python run_network.py

# Test plugin loading, dynamic binding, and unloading:
python run_plugin_lifecycle.py

# Test Top-Level Await error handling in JS:
python run_tla_safeguard.py
```

