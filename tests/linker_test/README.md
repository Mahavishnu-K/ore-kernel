# Dynamic Linker Testing (`linker_test`)

## Purpose
This directory contains the test suite for validating ORE's **WebAssembly Dynamic Linking** capabilities within the Zero-Trust Sandbox. It ensures that the kernel can safely load and execute external WebAssembly modules (plugins) at runtime from within a host WASM process.

## Architecture & Existence
When AI agents need to load dynamic tools on the fly, they shouldn't need to recompile their entire environment. ORE supports dynamic linking via custom imports (`ore_dlopen`, `ore_dlsym`).

This folder contains the core C files to validate this boundary:
- **`main.c`**: The host WASM agent. It boots up, calls the ORE kernel to load a `.so` (WASM) plugin, resolves a function pointer (`calculate`), and executes it. It also demonstrates WASM table index hijacking to allocate function slots dynamically.
- **`plugin.c`**: A simple C extension that exports the `calculate` function. It gets compiled into `plugin.wasi.so` and loaded by `main.c`.

## How it works with ORE
1. The AI Agent executes the `main.wasm` cartridge.
2. The WASM tool (running on behalf of the agent) requests a dynamic library via `ore_dlopen("plugin.wasi.so")`.
3. The ORE Kernel intercepts this call, validates permissions, injects the plugin's code into the agent's WebAssembly linear memory, and updates the function table.
4. The agent resolves the symbol and executes the native plugin securely, without breaking the sandbox boundaries.
