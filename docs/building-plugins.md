# Building Dynamic Plugins (`ore-ld`)

**Difficulty:** Medium
**Required SDKs:** `ore-sys` (Rust) or `syskit` (`ore.h` for C/C++, `ore.zig` for Zig, both injected automatically by `ore mktool`)

Memory Fusion lets a **host tool** load pre-compiled WebAssembly plugins (`.wasi.so`) at runtime with the Ore dynamic linker, `ore-ld`. The plugin is linked into the host's own linear memory, so the host passes it pointers instead of serialized data, and the plugin mutates the host's buffers in place.

For how the linker works internally, see [Polyglot Memory Fusion](./kernel-internals/polyglot-memory-fusion.md).

---

## How Memory Fusion Works

When a host calls `ore_dlopen("plugin.wasi.so")`:

1. **Memory slab:** the kernel gives the plugin a 1 MiB data region: a slab recycled from a closed plugin (wiped to zero first), or 16 new pages from `memory.grow`. Either way, it never overlaps the host's own heap.
2. **ABI globals:** the kernel creates the `__memory_base` and `__table_base` globals that LLVM's `-fPIC` code expects.
3. **Shared memory and table:** the plugin is instantiated against the host's own `memory` and `__indirect_function_table`.
4. **Binding:** each `ore_dlsym` adds one slot to the host's function table and returns its index, which the host calls as an ordinary function pointer.
5. **Unloading:** `ore_dlclose` nulls those slots (stale calls trap) and recycles the slab.

---

## The Host API at a Glance

| Operation | C / C++ (`ore.h`) | Rust (`ore-sys`) | Zig (`ore.zig`) |
|---|---|---|---|
| Load | `ore_load("x.wasi.so")` → handle, `<= 0` on failure | `Plugin::load("x.wasi.so")` → `Result` | `try ore.Plugin.load("x.wasi.so")` |
| Bind | `ORE_BIND(h, name, ret, args...)` → `NULL` on failure | `ore_bind!(p, name, fn(args) -> ret)` (panics on failure) or `p.get_func::<T>("name")` → `Result` | `try p.bind(fn (args) ret, "name")` |
| Unload | `ore_close(h)` → `0` on success | `p.close()` → `Result`, or just let `p` drop | `try p.close()` |
| Last error | `ore_error(buf, len)` → bytes written | `Plugin::last_error()` → `Option<String>` | `ore.lastError()` → `?[]const u8` |

Errors are reported by the kernel and cleared once read. In Rust, dropping a `Plugin` closes it, so **keep the `Plugin` alive as long as you use functions bound from it**: calling them after it's dropped traps.

---

## Rust Host & Plugins (`ore-sys`)

### 1. Writing the Plugin (`encryptor.rs`)

Use the `ore_export!` macro. It applies `#[no_mangle]` and `extern "C"`, so the function follows the C ABI and can be called from any language. Plugins are `no_std`: the `plugin` feature of `ore-sys` provides the panic handler.

```rust
#![no_std]
use ore_sys::ore_export;

// Mutates the host's memory directly.
ore_export! {
    fn xor_cipher(data_ptr: *mut u8, len: usize, key: u8) {
        let slice = unsafe { core::slice::from_raw_parts_mut(data_ptr, len) };
        for byte in slice.iter_mut() {
            *byte ^= key;
        }
    }
}
```

### 2. Writing the Host (`crypto_tool.rs`)

```rust
use ore_sys::host::Plugin;
use ore_sys::ore_bind;

fn main() {
    // 1. Load the plugin (ore_dlopen)
    let plugin = match Plugin::load("encryptor.wasi.so") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("load failed: {:?}", Plugin::last_error());
            return;
        }
    };

    // 2. Resolve the symbol (ore_dlsym) into a typed function pointer
    ore_bind!(plugin, xor_cipher, fn(*mut u8, usize, u8));

    // 3. Zero-copy call: the plugin mutates the host's heap in place
    let mut payload = String::from("CLASSIFIED_ENTERPRISE_DATA").into_bytes();
    xor_cipher(payload.as_mut_ptr(), payload.len(), 0xAA);
    println!("Encrypted Payload: {:?}", payload);

    // 4. Unload explicitly (or let `plugin` drop at the end of scope)
    plugin.close().expect("close failed");
}
```

---

## C, C++, and Zig (`syskit`)

`ore mktool` injects `ore.h` (C/C++) or `ore.zig` (imported as `ore_sys`) when you build with `--host` or `--shared`. Because everything crosses the linker boundary through the C ABI, **any host can load any plugin**: a Rust host can load a C plugin, a C host can load a Zig plugin, and so on.

### Example: C Host loading a C++ Plugin

```c
#include <stdio.h>
#include <string.h>
#include <ore.h>

int main() {
    // 1. Load the plugin
    OrePlugin plugin = ore_load("sanitizer.wasi.so");
    if (plugin <= 0) {
        char err[256];
        int n = ore_error(err, sizeof(err) - 1);
        err[n > 0 ? n : 0] = '\0';
        printf("Failed to load plugin: %s\n", err);
        return 1;
    }

    // 2. Resolve and cast the function pointer
    // Syntax: ORE_BIND(plugin_handle, func_name, return_type, arg_types...)
    ORE_BIND(plugin, clean_string, void, char*, size_t);
    if (!clean_string) return 1;

    // 3. Zero-copy call
    char buffer[] = "USER_INPUT: DROP TABLE users;";
    clean_string(buffer, strlen(buffer));
    printf("Sanitized: %s\n", buffer);

    // 4. Unload
    ore_close(plugin);
    return 0;
}
```

### Writing a C/C++ Plugin

Mark exported functions with `ORE_PLUGIN`, which exports them without name mangling (it adds `extern "C"` automatically in C++). `ore mktool --shared` defines `ORE_PLUGIN_MODE` for you, so the header leaves out the host API.

```cpp
// sanitizer.cpp
#include <ore.h>

ORE_PLUGIN void clean_string(char* data, size_t len) {
    for (size_t i = 0; i < len; i++) {
        if (data[i] == ';') {
            data[i] = '_'; // Strip dangerous SQL characters in place
        }
    }
}
```

### Zig Host and Plugin

```zig
// parser.zig (plugin): a plain `export fn` is enough
export fn count_error_flags(data_ptr: [*]const u8, len: u32, target_flag: u32) u32 {
    var count: u32 = 0;
    var i: u32 = 0;
    while (i < len) : (i += 1) {
        if (data_ptr[i] == @as(u8, @intCast(target_flag))) count += 1;
    }
    return count;
}
```

```zig
// iot_tool.zig (host)
const std = @import("std");
const ore = @import("ore_sys"); // injected by ore mktool --host

pub fn main() !void {
    var plugin = try ore.Plugin.load("parser.wasi.so");
    defer plugin.close() catch {};

    const parse = try plugin.bind(fn ([*]const u8, u32, u32) u32, "count_error_flags");

    const sensor_data = [_]u8{ 0x00, 0xFF, 0x01, 0x00, 0xFF, 0xFF, 0x02, 0x00 };
    const errors = parse(&sensor_data, sensor_data.len, 0xFF);
    std.debug.print("Found {} critical errors\n", .{errors});
}
```

### What to pass across the boundary

Pass **pointers and lengths to plain data**: bytes, integers, floats, or `repr(C)` / C structs. Don't pass language-specific objects such as `std::string`, a Rust `String`, or a Zig slice header. The other side would interpret their layout differently.

---

## Compiling with `ore mktool`

`ore mktool` handles all the linker flags and SDK injection. It accepts a single file or a project directory (for example, a Cargo crate).

### Building the Host

Use `--host` when your tool **loads** plugins. It exports the host's function table to the kernel so `ore-ld` can extend it. Tools that don't use plugins don't need the flag.

```bash
ore mktool host_agent.rs --host     # single file
ore mktool . --host                 # project directory
```

Hosts are written to `~/.ore/tools/<name>.wasm` and run through `POST /execute` like any other tool. They must be listed in the agent's `allowed_tools`.

### Building the Plugin

Use `--shared` to build a position-independent WebAssembly shared object:

```bash
ore mktool encryptor.rs --shared    # single file
ore mktool . --shared               # project directory
```

Plugins are written to `~/.ore/plugins/<name>.wasi.so`, which is where `ore_dlopen` looks for them.

### What the flags do

| Language | `--shared` (plugin) | `--host` |
|---|---|---|
| Rust | target `wasm32-unknown-unknown`, `-C relocation-model=pic -C link-arg=-shared` | target `wasm32-wasip1`, `--export-dynamic --import-table` |
| C / C++ | `zig cc/c++ -target wasm32-freestanding -shared -fPIC -nostdlib -DORE_PLUGIN_MODE` | `zig cc/c++ -target wasm32-wasi -Wl,--export-dynamic -Wl,--import-table` |
| Zig | `-target wasm32-freestanding -dynamic` | `-target wasm32-wasi -rdynamic --import-symbols --import-table`, with `ore.zig` as module `ore_sys` |

**C++ note:** WASI's libc++ has no exception support, so all C++ builds use `-fno-exceptions`. `try`/`catch` won't compile, and anything that would have thrown (such as an out-of-range `vector::at`) aborts instead, which ends the tool with a trap.

**Plugin languages:** plugins must be written in Rust, C, C++, or Zig. Python, JavaScript, and Go can't be plugins because their runtimes need to own the whole linear memory.

---

## Testing Your Plugin

The repository has working host/plugin pairs to copy from:

- [`tests/memory_fusion/`](../tests/memory_fusion/): same-language pairs, plus lifecycle tests (`dlclose`, `dlerror`, slab reuse, stale-pointer trap)
- [`tests/cross_lang_memory_fusion/`](../tests/cross_lang_memory_fusion/): cross-language pairs, a load/unload loop, and a hot-swap
- [`examples/wasm_agents/run_plugin_lifecycle.py`](../examples/wasm_agents/run_plugin_lifecycle.py): runs the lifecycle tests through the kernel and checks the results

---

**← Back to:** [Documentation Index](./README.md)
