# Polyglot Memory Fusion (`ore-ld`)

> A `dlopen`-style dynamic linker that loads WebAssembly plugins into a running host tool's memory.

**Source:** [`ore-core/src/linker/`](../../ore-core/src/linker/) · **Status:** Alpha

---

## The Problem

Sandboxed tools exchange data over stdin/stdout. That's fine for most work, but when a host tool holds a large buffer and needs another component to transform it, serializing the buffer out and back dominates the cost.

The usual alternatives each give something up:

- **Native FFI** shares memory and is fast, but a crash in the library takes down the host, and the library must be rebuilt for each OS and architecture.
- **Microservices** are isolated, but every call pays for serialization and a network hop.

`ore-ld` lets a host WebAssembly module load a plugin into **its own linear memory** at runtime and call it through a function pointer. The data is never copied.

---

## Building Blocks

| File | Role |
|---|---|
| [`mod.rs`](../../ore-core/src/linker/mod.rs) | Registers the four host calls (`ore_dlopen`, `ore_dlsym`, `ore_dlclose`, `ore_dlerror`) in the `env` import module |
| [`syscalls.rs`](../../ore-core/src/linker/syscalls.rs) | The implementations of those calls |
| [`mmu.rs`](../../ore-core/src/linker/mmu.rs) | Grows the host's memory for plugin data, and builds the `__memory_base` / `__table_base` globals |
| [`linker_state.rs`](../../ore-core/src/linker/linker_state.rs) | Per-execution state: loaded plugins, next handle, last error, free memory slabs |

Plugins are built position-independent (`ore mktool --shared` → `-fPIC -shared`), so they don't assume they're loaded at address 0. They import `memory`, `__indirect_function_table`, `__memory_base`, and `__table_base` from `env`, following the LLVM WebAssembly dynamic-linking convention. Hosts are built with `--export-dynamic --import-table` (`ore mktool --host`) so the kernel can extend their function table.

---

## The Host Calls

All four are imported from the `env` module. Strings are passed as `(pointer, length)` into the host's memory.

### `ore_dlopen(name_ptr, name_len) -> i32`

1. Reads the plugin from `~/.ore/plugins/<name>` and compiles it.
2. Takes a **memory slab** for the plugin's data:
   - reuses a slab freed by an earlier `ore_dlclose`, **zero-filling it first** so the new plugin can't read the old one's data and zero-initialized statics (`.bss`) really start at zero; or
   - grows the host's linear memory by **16 pages (1 MiB)**. The old end of memory becomes the slab.
3. Sets `__memory_base` to the slab start, and `__table_base` to the current size of the host's function table.
4. Instantiates the plugin against the **host's** `memory` and `__indirect_function_table`.
5. Runs the plugin's `__wasm_call_ctors`, if present.
6. Records the plugin under a new handle and returns it (`≥ 1`).

On failure it returns `-1`, stores a reason for `ore_dlerror`, and returns the slab to the free list if one was already taken.

### `ore_dlsym(handle, name_ptr, name_len) -> i32`

Looks up the export on that plugin, **grows the host's function table by one slot**, stores the function there, records the slot against the handle, and returns the slot index. WebAssembly enforces control-flow integrity: code can only call functions through the table, never jump to an arbitrary address. So this index is the only way the host can reach the plugin.

On failure (unknown handle, missing symbol, bad string) it returns `0` (`NULL`) and stores a reason.

### `ore_dlclose(handle) -> i32`

1. Removes the handle.
2. Sets every table slot handed out by `ore_dlsym` for that handle to `null`. Calling a stale function pointer now **traps** instead of running freed code.
3. Pushes the plugin's slab onto the free list for the next `ore_dlopen`.

Returns `0`, or `-1` (with a reason) for an unknown or already-closed handle.

### `ore_dlerror(buf_ptr, buf_len) -> i32`

Copies the last error message into the host's buffer (truncated to `buf_len`) and returns the number of bytes written, or `0` if there's no error. The message is cleared once it has been delivered, so a second call returns `0`.

---

## Zero-Copy Execution

After `ore_dlopen`, the host and the plugin are two modules **sharing one linear memory**. The plugin's code and globals are its own, but every load and store it makes goes to the host's memory.

In wasm32, a pointer is just a 32-bit offset into linear memory. Because both sides index the same memory, a pointer produced by the host means exactly the same bytes to the plugin:

```
Host linear memory (one block, shared)
┌───────────────────────┬────────────────────────┬────────────────────────┐
│ Host stack + heap     │ ...                    │ Plugin data slab (1MiB)│
│   char data[] ───┐    │                        │ (__memory_base)        │
└──────────────────┼────┴────────────────────────┴────────────────────────┘
                   │
   xor_cipher(data, len, key)   ← C host passes the offset (an i32)
                   │
                   ▼
   Rust plugin builds a &mut [u8] at that same offset and XORs it in place
```

A call goes through these steps:

1. The host passes a pointer (an `i32` offset) and a length as ordinary arguments.
2. `call_indirect` jumps into the plugin through the table slot from `ore_dlsym`. It's a normal WebAssembly call: no host round-trip, no serialization, no copy.
3. The plugin reads and writes the host's bytes directly.
4. When the call returns, the host sees the mutated data immediately. Nothing needs copying back.

This is the same mechanism as calling a native shared library, but with WebAssembly's bounds checks: an access outside the linear memory traps instead of corrupting the process.

### Crossing the language boundary

Both sides communicate through the **C ABI** as it's defined for wasm32: `i32`/`i64`/`f32`/`f64` values, and pointers as `i32` offsets. Any language that can export and import C-ABI functions can take part, whichever language built the other side. The test suites show this with each pairing:

| Host → Plugin | What crosses the boundary |
|---|---|
| C → Rust (`c_uses_rust.c` + `encryptor.rs`) | A C stack buffer; Rust XOR-encrypts it in place |
| Rust → C++ (`rust_uses_cpp.rs` + `sanitizer.cpp`) | A Rust `Vec<u8>`'s buffer; C++ compacts the spaces and returns the new length, and Rust truncates the vector to it |
| Zig → C (`zig_uses_c.zig` + `stats.c`) | A Zig `[]f64`; C reads it as `double*` and returns the variance. Both sides agree on IEEE-754 `f64` |
| C++ → Rust (`cpp_hotswaps_rust.cpp` + `pricing_v1/v2.rs`) | Scalars in and out, across a live hot-swap |

The rule for authors: pass **pointers and lengths to plain data** (bytes, integers, floats, `repr(C)` structs). Don't pass language-specific objects such as a `std::string`, a Rust `String`, or a Zig slice header. Each side would interpret their layout differently.

---

## SDKs

The SDKs hide the pointer and length handling:

| Language | Host API | Plugin API |
|---|---|---|
| C / C++ (`ore.h`, injected by `ore mktool`) | `ore_load`, `ORE_BIND`, `ore_close`, `ore_error` | `ORE_PLUGIN` (adds `extern "C"` in C++) |
| Rust (`ore-sys` crate) | `Plugin::load`, `ore_bind!`, `get_func`, `close`, `Plugin::last_error`, and `Drop` closes the plugin automatically | `ore_export!` (with the `plugin` feature, `no_std`) |
| Zig (`ore.zig`, injected as `ore_sys`) | `Plugin.load`, `bind`, `close`, `lastError` | `export fn` |

Example (C host, Rust plugin), from [`tests/cross_lang_memory_fusion/c_uses_rust.c`](../../tests/cross_lang_memory_fusion/c_uses_rust.c):

```c
OrePlugin plugin = ore_load("encryptor.wasi.so");
ORE_BIND(plugin, xor_cipher, void, char*, int, unsigned char);

char data[] = "C_LOVES_RUST_MEMORY_FUSION";
xor_cipher(data, sizeof data - 1, 0xAA);   // Rust mutates C's buffer in place

ore_close(plugin);
```

See [Building Plugins](../building-plugins.md) for the full workflow.

---

## Guarantees and Non-Guarantees

- ✅ A plugin can only touch the host's linear memory and the imports Ore gives it. It can't reach the kernel, the host OS, or other agents.
- ✅ Faults inside a plugin (out-of-bounds access, a call through a null slot) are WebAssembly traps. The execution is aborted and the kernel keeps running.
- ✅ The same `.wasi.so` runs on Linux, macOS, and Windows.
- ⚠️ A plugin is **not** isolated from its host: they share one linear memory, so a buggy plugin can corrupt host data. Only load plugins you trust as much as the host.

## Hot-Swapping

A long-running host can `ore_dlopen` a new version of a plugin, re-bind its symbols, and `ore_dlclose` the old handle. The host's state lives in the host's memory, so it's untouched by the swap. The new version usually lands in the old version's recycled slab.

## Why Only Rust, C, C++, and Zig?

These toolchains can emit position-independent WebAssembly that uses an *imported* memory and table. Garbage-collected runtimes (Python, JavaScript, Go) expect to own the whole linear memory, so they can't share one with another module's allocator. They use regular `.wasm` tools instead.

## Current Limits

- Each plugin gets a fixed 1 MiB data slab, whatever its size.
- `ore_dlclose` doesn't run plugin destructors.
- wasmtime keeps a closed plugin's instance alive until the execution ends. Only its handle, table slots, and memory slab are released.
- Function pointers a plugin hands to the host directly, rather than through `ore_dlsym`, are not nulled on close.
- Symbols aren't resolved between plugins.
- Plugins are compiled on every `ore_dlopen` rather than AOT-cached.
- The host and all its plugins share the wasm32 limit of 4 GiB of linear memory.

## Tests

- [`tests/memory_fusion/`](../../tests/memory_fusion/) and [`tests/cross_lang_memory_fusion/`](../../tests/cross_lang_memory_fusion/) contain host/plugin pairs across the supported languages, including lifecycle tests for `dlclose`, `dlerror`, slab recycling, and stale-pointer traps.
- [`examples/wasm_agents/run_plugin_lifecycle.py`](../../examples/wasm_agents/run_plugin_lifecycle.py) runs the lifecycle tests through the kernel and checks their output.

---

**← Back to:** [Kernel Internals Index](./README.md)
