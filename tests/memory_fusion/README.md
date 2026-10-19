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

### Plugin lifecycle tests (`ore_dlclose` / `ore_dlerror`)

These pairs test unloading, error reporting, and memory-slab recycling. Every host prints a `PASS` / `FAIL` line per check and ends with `ALL CHECKS PASSED` (except the stale-pointer test, which must end in a trap).

- **C to C: lifecycle (`lifecycle_tool.c` + `counter.c`)**: The plugin keeps a counter in `.bss`. The host checks that:
  - a missing plugin fails and `ore_error` explains why, then reads back `(none)` the second time;
  - a missing symbol binds to `NULL` and `ore_error` explains why;
  - `ore_close` succeeds once and fails on a second call with the same handle;
  - reloading reuses the **same slab address**, and the counter restarts at **1**, proving the recycled slab was zeroed (it would print 4 otherwise). The kernel log shows `Reusing recycled memory slab at base: …`.
- **Rust to Rust: hot-swap (`hotswap_tool.rs` + `pricing_v1.rs` / `pricing_v2.rs`)**: The host quotes an order book with v1 (2× markup, total **850**), closes it explicitly with `close()`, swaps in v2 (3× markup, total **1275**), and lets `Drop` close v2. Its `history` vector must hold all six quotes from both models. It also checks `Plugin::last_error()` after a failed load and a failed `get_func`.
- **Zig to Zig: stale pointer (`stale_pointer_tool.zig` + `beacon.zig`)**: The host calls `ping(41)` (expects 42), closes the plugin, checks that a second close fails with `PluginCloseFailed` and `lastError()`, then calls the old `ping` pointer. **Expected result: the execution ends in a WebAssembly trap** (an uninitialized/null table element) and the kernel keeps running. If you see `FAIL stale pointer still ran`, the table slots were not nulled.

```bash
# Plugins → ~/.ore/plugins/
ore mktool counter.c --shared
ore mktool pricing_v1.rs --shared
ore mktool pricing_v2.rs --shared
ore mktool beacon.zig --shared

# Hosts → ~/.ore/tools/
ore mktool lifecycle_tool.c --host
ore mktool hotswap_tool.rs --host
ore mktool stale_pointer_tool.zig --host
```

*(Note: These tools can be mixed and matched. A Rust Host can dynamically load a C plugin and vice-versa, as long as they agree on the C-ABI function signature!)*

## How it works with ORE
1. The developer uses the **ORE SDKs** (`ore-sys/` for Rust, `syskit/` for C/Zig) to compile the plugin and the host.
2. The plugin uses the SDK macro (e.g., `ore_export!`) to expose its function.
3. The Host Agent uses the SDK to load the plugin (`Plugin::load()`) and bind the function pointer (`ore_bind!`).
4. When executed, the ORE Kernel allocates isolated memory pages for the plugin's variables using the internal MMU, but fuses the linear memory so both programs share the same heap!
