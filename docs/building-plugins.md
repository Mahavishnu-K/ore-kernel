# Building Dynamic Plugins (`ore-ld`)

**Difficulty:** Medium  
**Required SDKs:** `ore-sys` (Rust) or `syskit` (C, C++, Zig)

ORE's **Zero-Copy Cross-Language Memory Fusion** is powered by `ore-ld` (the ORE Dynamic Linker). It allows Host Agents to dynamically load pre-compiled WebAssembly plugins (`.wasi.so`) at runtime. 

Unlike traditional Docker containers or standard WASM runtimes that force you to serialize data to JSON and pipe it over STDOUT/STDIN, ORE injects the exact same linear memory space into both the Host Agent and the Dynamically Loaded Plugin. This allows plugins to mutate the host's RAM directly in-place, achieving true bare-metal FFI performance with zero IPC overhead.

---

## How Memory Fusion Works

When a Host Agent calls `ore_dlopen("plugin.wasi.so")`:
1. **Memory Allocation:** The Kernel's MMU allocates isolated pages (via `memory.grow`) for the plugin's data segment, preventing it from overwriting the Host's malloc heap.
2. **ABI Forging:** The Kernel injects the `__memory_base` and `__table_base` globals required by the LLVM `-fPIC` (Position Independent Code) C-ABI.
3. **Table Hijacking:** The Kernel dynamically expands the Wasmtime `__indirect_function_table` and inserts the plugin's function pointers.
4. **Shared RAM:** The Kernel injects the exact same `memory` export into the plugin. The plugin now lives inside the Host's RAM.

---

## Rust Host & Plugins (`ore-sys`)

The `ore-sys` crate provides high-level macros that hide the raw system calls and unsafe pointer math.

### 1. Writing the Plugin (`encryptor.rs`)
To write a plugin, use the `ore_export!` macro. This macro automatically applies `#[no_mangle]` and `extern "C"` to ensure the function adheres to the standard C-ABI, making it callable from any language.

```rust
#![no_std]
use ore_sys::ore_export;

// This function mutates the Host's RAM directly!
ore_export! {
    fn xor_cipher(data_ptr: *mut u8, len: usize, key: u8) {
        // Reconstruct a mutable Rust slice directly from the raw pointer
        let slice = unsafe { core::slice::from_raw_parts_mut(data_ptr, len) };
        
        // Mutate the host's memory in-place
        for byte in slice.iter_mut() {
            *byte ^= key;
        }
    }
}
```

*Note: Compile plugins using `ore mktool encryptor.rs` which automatically targets `wasm32-wasi` and outputs `encryptor.wasi.so` into the `~/.ore/plugins` directory.*

### 2. Writing the Host Agent (`crypto_tool.rs`)
The Host Agent allocates memory and passes a pointer to the plugin. Use `Plugin::load` to invoke the `dlopen` syscall, and `ore_bind!` to invoke `dlsym` and safely transmute the returned integer index into a callable Rust closure.

```rust
use ore_sys::host::Plugin;
use ore_sys::ore_bind;

fn main() {
    // 1. Dynamically load the plugin into the Sandbox
    let plugin = Plugin::load("encryptor.wasi.so").expect("Failed to load plugin!");

    // 2. Resolve the symbol and bind it to a native Rust function pointer
    // Syntax: ore_bind!(plugin_instance, function_name, function_signature)
    ore_bind!(plugin, xor_cipher, fn(*mut u8, usize, u8));

    // Allocate memory on the Host's heap
    let mut payload = String::from("CLASSIFIED_ENTERPRISE_DATA").into_bytes();
    
    // 3. Zero-Copy Execution (Mutates payload in-place!)
    unsafe { 
        xor_cipher(payload.as_mut_ptr(), payload.len(), 0xAA); 
    };

    println!("Encrypted Payload: {:?}", payload);
}
```

---

## C, C++, and Zig (`syskit`)

If you are writing the Host Agent or Plugin in C, C++, or Zig, you must use the raw system calls provided by the `syskit` SDK headers (`ore.h` or `ore.zig`). 

Because ORE enforces the standard C-ABI across the linker boundary, **a Rust host can load a C plugin, and a C host can load a Zig plugin.**

### Example: C Host Agent loading a C++ Plugin
Include `ore.h` to access the SDK.

```c
#include <stdio.h>
#include <string.h>
#include <ore.h>

int main() {
    // 1. Request the dynamic library from the ORE Kernel using the elegant API
    OrePlugin plugin = ore_load("sanitizer.wasi.so");
    if (plugin <= 0) {
        printf("Failed to load plugin!\n");
        return 1;
    }

    // 2. Automatically resolve and cast the function pointer!
    // Syntax: ORE_BIND(plugin_handle, func_name, return_type, arg1_type, arg2_type...)
    ORE_BIND(plugin, clean_string, void, char*, size_t);

    // 3. Execute in-place Memory Fusion
    char buffer[] = "USER_INPUT: DROP TABLE users;";
    
    clean_string(buffer, strlen(buffer));
    
    printf("Sanitized: %s\n", buffer);
    return 0;
}
```

### Writing a C/C++ Plugin
To write a plugin in C or C++, simply mark the function with the `ORE_PLUGIN` macro so it is exported into the WASM binary without name-mangling.

```cpp
// sanitizer.cpp
#define ORE_PLUGIN_MODE
#include <ore.h>

ORE_PLUGIN void clean_string(char* data, size_t len) {
    for (size_t i = 0; i < len; i++) {
        if (data[i] == ';') {
            data[i] = '_'; // Strip dangerous SQL characters in-place
        }
    }
}
```

---

## Compiling with `ore mktool`
Once you have written your Host Agent and your Plugin, you must compile them into WebAssembly cartridges using the ORE toolchain. ORE handles all the complex `-fPIC` linker flags and SDK bindings automatically.

Depending on if you are compiling a single file or a full project directory (like a Cargo crate or Go module), use the following commands:

### Forging the Host Agent
The `--host` flag must be used when your WASM executable intends to *load* a plugin via `ore_load`. This flag tells the ORE compiler to surrender the Wasmtime function table to the OS so the kernel can hijack it. (If your tool doesn't use Memory Fusion plugins, just use `ore mktool` without the `--host` flag).
```bash
# Compile a single file
ore mktool host_agent.rs --host

# Compile a full project directory (e.g., Cargo/NPM project)
ore mktool . --host
```
*(Note: ORE automatically outputs `--host` executables into your `~/.ore/tools` directory so they can be securely executed by the sandbox!)*

### Forging the Dynamic Plugin
The `--shared` flag compiles your code into a dynamically loadable WebAssembly Shared Object (`.wasi.so`). This is required for it to be loaded via `ore_dlopen`.
```bash
# Compile a single file plugin
ore mktool encryptor.rs --shared

# Compile a full project directory plugin
ore mktool . --shared
```
*(Note: ORE automatically outputs `.wasi.so` plugins into your `~/.ore/plugins` directory so they can be instantly resolved by the kernel!)*

---

**← Back to:** [Documentation Index](./README.md)
