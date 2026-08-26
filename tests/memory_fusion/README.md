# Cross-Language Memory Fusion (`memory_fusion`)

## Purpose
This directory contains the test suite validating **Zero-Copy Cross-Language Memory Fusion**, which is the most powerful capability of the ORE Dynamic Linker (`ore-ld`). 

Unlike traditional microservices or standard WASM architectures that force you to serialize data to JSON and pipe it over STDOUT/STDIN, ORE's linker architecture injects the exact same linear memory space into both the Host Agent and the Dynamically Loaded Plugin. This allows plugins to mutate the host's RAM directly in-place.

## Architecture & Existence
This folder contains various Agent/Plugin pairs across different languages to prove language-agnostic FFI (Foreign Function Interface) execution inside the WebAssembly Sandbox:

- **Rust to Rust (`crypto_tool.rs` + `encryptor.rs`)**: A Rust Host Agent allocates a string in its memory, dynamically loads the `encryptor.wasi.so` plugin, and passes the raw memory pointer. The plugin reconstructs the slice and applies an XOR cipher directly to the host's memory.
- **C to C (`finance_tool.c` + `stats.c`)**: Demonstrates native C-struct manipulation across the dynamic linker boundary.
- **C++ to C++ (`data_tool.cpp` + `sanitizer.cpp`)**: Validates that C++ objects can be passed by reference to a WASM plugin for high-speed data sanitization.
- **Zig to Zig (`iot_tool.zig` + `parser.zig`)**: Tests Zig's unique memory allocators functioning across the linker boundary.

*(Note: These tools can be mixed and matched. A Rust Host can dynamically load a C plugin and vice-versa, as long as they agree on the C-ABI function signature!)*

## How it works with ORE
1. The developer uses the **ORE SDKs** (`ore-sys/` for Rust, `syskit/` for C/Zig) to compile the plugin and the host.
2. The plugin uses the SDK macro (e.g., `ore_export!`) to expose its function.
3. The Host Agent uses the SDK to load the plugin (`Plugin::load()`) and bind the function pointer (`ore_bind!`).
4. When executed, the ORE Kernel allocates isolated memory pages for the plugin's variables using the internal MMU, but fuses the linear memory so both programs share the same heap!
