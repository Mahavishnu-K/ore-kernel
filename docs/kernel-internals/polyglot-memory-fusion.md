# Polyglot Memory Fusion (`ore-ld`)

> **Source:** `ore-core/src/linker/`  
> **Status:** Active, Production-Ready  

## The Problem with Microservices in AI

In standard backend architectures, if a Python agent needs to compress a 5GB JSON payload using a high-performance C++ tool, it must:
1. Serialize the payload to a JSON string or Protobuf.
2. Send it over a network socket (or `STDIN/STDOUT` pipe).
3. The C++ tool allocates its own RAM, deserializes the JSON, processes it, serializes the result, and pipes it back.
4. The Python agent deserializes the result.

This **Serialization Tax** is unacceptable for high-frequency, memory-intensive AI workloads. 

## The ORE Solution: Zero-Copy Memory Fusion

ORE introduces `ore-ld`, a custom POSIX-compliant Dynamic Linker built directly into the WebAssembly Kernel. Instead of isolating tools via network sockets, ORE physically injects them into the agent's live RAM.

### 1. Position Independent Code (fPIC)
Developers compile their plugins as WebAssembly Shared Objects (`.wasi.so`). The `ore mktool --shared` compiler flag forces LLVM to generate Position Independent Code. This means the plugin does not assume it owns the memory from address `0x0`. Instead, it relies on global variables (`__memory_base` and `__table_base`) to know where it is loaded.

### 2. The Custom MMU (`linker/mmu.rs`)
When a Host Agent calls `ore_dlopen("plugin.wasi.so")`, the ORE Kernel halts execution. The internal Memory Management Unit (MMU):
- Inspects the plugin's `.data` segments.
- Calls `memory.grow` on the Host Agent's WebAssembly linear memory to allocate fresh pages.
- Calculates the offset and dynamically injects the `__memory_base` integer into the plugin.
- Injects the plugin's data directly into the Host's RAM.

### 3. Function Table Hijacking (`trap_ore_dlsym`)
WebAssembly enforces strict Control Flow Integrity (CFI). A Host Agent cannot arbitrarily jump to an unverified memory address.
When the Host Agent calls `ore_dlsym("my_function")`, ORE:
- Dynamically expands the Host Agent's `__indirect_function_table`.
- Maps the plugin's function pointer into the newly created table slots.
- Returns the exact integer index to the Host Agent.

### 4. Zero-Copy Execution
Because the Plugin and the Host Agent now physically share the exact same `memory` export block, they can pass raw pointers to each other. 
- A **C Host** can allocate a `char*` on its stack and pass the pointer to a **Rust Plugin**.
- The Rust Plugin can mutate the memory in-place.
- The C Host reads the mutated memory instantly.

Zero serialization. Zero network overhead. Bare-metal nanosecond latency.

## Language Agnosticism (The C-ABI)

Memory Fusion transcends language barriers by strictly adhering to the standard C-ABI (Application Binary Interface). 
Regardless of whether a Host or Plugin is written in Rust, C++, Zig, or C, they all compile to WebAssembly and communicate via standard C pointers (32-bit integers in WASM32) and basic types (`i32`, `f32`, `f64`).

This means a Zig agent can allocate an array of floats and pass the raw pointer to a C library, which calculates the standard variance and returns the result, proving perfect IEEE-754 compatibility across the fusion boundary.

## Hot-Swappable Brain Implants

Because `ore-ld` links code dynamically at runtime, an AI Agent can hot-swap its own logic without losing its internal state. An agent can hold 10GB of vector embeddings in RAM, dynamically unload an old plugin, load an upgraded plugin, and continue processing instantly without rebooting or rebuilding memory state. 

---

**← Back to:** [Documentation Index](../README.md)
