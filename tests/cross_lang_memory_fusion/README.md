# Cross-Language Memory Fusion Test Suite

This directory contains tests proving ORE's **Polyglot Memory Fusion** via the `ore-ld` dynamic linker. 

## What is this?
Normally, if an agent written in Rust wants to use a C++ tool to sanitize data, it must serialize the memory into JSON, pipe it over standard I/O (STDOUT/STDIN), and deserialize it on the other side. This incurs a massive latency penalty for large enterprise workloads.

**Memory Fusion** bypasses this entirely. By strictly adhering to the WebAssembly C-ABI, ORE physically injects the exact same linear memory space into both a Host Agent and a dynamically loaded Plugin (`.wasi.so`). 

## Why is it important?
These tests demonstrate the absolute power of ORE for enterprise AI workloads:
1. **Zero-Copy Performance**: Massive chunks of RAM (like Gigabytes of text, arrays, or order-books) can be handed from one programming language to another with zero serialization and zero bytes copied.
2. **Language Agnosticism**: AI agents can leverage legacy, high-performance logic written in *any* systems language, seamlessly fusing C, C++, Rust, and Zig memory without FFI nightmares.
3. **Hot-Swappable Brain Implants**: Because the plugins are dynamically linked at runtime (`ore_load`), logic can be swapped without losing the agent's internal memory state.

## The Tests

### 1. C uses Rust (`c_uses_rust.c`)
- **Plugin Required:** `encryptor.wasi.so` (Compiled from Rust)
- **Mechanics:** The C Host Agent allocates a character array on its stack. It uses `ORE_BIND` to hijack the Rust `xor_cipher` function. The Rust plugin receives the raw pointer and encrypts the C string *in-place*. 

### 2. Rust uses C++ (`rust_uses_cpp.rs`)
- **Plugin Required:** `sanitizer.wasi.so` (Compiled from C++)
- **Mechanics:** The Rust Host Agent creates a standard heap-allocated `String` with messy spacing. It uses `ore_bind!` to load the C++ `sanitize_spaces` function. The C++ plugin operates on the Rust `Vec<u8>` pointer, cleans the bytes, and returns the new length so Rust can truncate it.

### 3. Zig uses C (`zig_uses_c.zig`)
- **Plugin Required:** `stats.wasi.so` (Compiled from C)
- **Mechanics:** The Zig Host Agent creates an array of `f64` floats (market data). It dynamically binds to the C `calculate_variance` function. The C plugin calculates standard variance reading the Zig slice natively.

## How to run the tests

1. Ensure all plugins are compiled to `~/.ore/plugins/`:
   ```bash
   ore mktool encryptor.rs --shared
   ore mktool sanitizer.cpp --shared
   ore mktool stats.c --shared
   ```

2. Compile these Host Agents to `~/.ore/tools/` using the `--host` flag (which surrenders the WebAssembly function table to the OS):
   ```bash
   ore mktool c_uses_rust.c --host
   ore mktool rust_uses_cpp.rs --host
   ore mktool zig_uses_c.zig --host
   ```

3. Run them via the ORE Sandbox!

---

## The Industry Significance (Why This Matters)

These tests prove that programs written in C, Rust, and Zig can dynamically load foreign binaries compiled from completely different languages, inject them into their own physical RAM, and execute them at bare-metal speed—with absolutely zero data copying, zero JSON parsing, and zero network overhead.

### 1. C Host -> Rust Plugin
- **The Proof:** The C program allocates memory on its stack and passes the pointer to Rust. Rust mutates the memory in-place (XOR cipher) and hands it back.
- **The Significance:** Rust's strict memory safety rules do not break when interfacing with C. It successfully operates on a raw C-pointer inside a shared WebAssembly linear memory space.

### 2. Rust Host -> C++ Plugin
- **The Proof:** Rust allocates a `String` (`Vec<u8>`). It hands the pointer to C++. C++ strips out the duplicate spaces and returns the new integer length. Rust uses that integer to safely `.truncate()` its vector.
- **The Significance:** Proves that two incredibly complex systems languages (C++ and Rust) can share ownership of a memory buffer and cooperatively resize it without corrupting the heap.

### 3. Zig Host -> C Plugin
- **The Proof:** Zig allocates an array of 64-bit floats (`f64`). It passes it to a C plugin expecting an array of `double`. C computes the statistical variance and returns it.
- **The Significance:** Proves that complex floating-point math transcends the language barrier perfectly. Zig and C agree on the exact byte-level representation of IEEE 754 floating-point numbers in WebAssembly.

### The Universal Execution Engine
In standard architectures, if a Python agent needs to execute C++ logic, it spins up a heavy container, sends data over an HTTP socket, waits for a server to process it, and sends it back—taking hundreds of milliseconds. 

The ORE Kernel accomplishes this in **nanoseconds**. It acts as a Universal Execution Engine:
- **The Kernel** manages the OS routing tables (`sandbox.rs`).
- **The Toolchain** forces the LLVM compilers to obey the memory physics (`ore mktool`).
- **The SDKs** hide the ugly pointers from the developers (`ore.h`, `ore-sys`, `ore.zig`).
