# ORE Kernel Test Suite

This directory contains the comprehensive test suites that validate the fundamental security boundaries, dynamic linking architectures, and compilation capabilities of the **ORE Kernel** and the `ore mktool` toolchain.

The test suite is organized into distinct categories based on the sub-systems they validate.

## Directory Structure

| Test Suite | Directory | Description |
|---|---|---|
| **Zero-Trust Sandbox Security** | `wasm_sandbox_tools/` | Validates absolute security boundaries: Network Layer 7 proxy (`network_interception.rs`), VFS isolation (`vfs_security_check.go`), and CPU Fuel limit trapping (`infinite_loop_trap.rs`). |
| **Full Project Compilation** | `project_tools/` | Validates `ore mktool` compiling complex, multi-file projects (Go modules, Node.js/TS packages, full Cargo workspaces, and Python workspaces with `requirements.txt`) into portable `.wasm` cartridges. |
| **Native Dynamic Linking** | `linker_test/` | Validates the low-level `ore-ld` dynamic linker and WebAssembly function table expansion via `ore_dlopen`. |
| **Zero-Copy Memory Fusion** | `memory_fusion/` | Validates standard FFI boundaries within the same language (e.g., Rust Host fusing with a Rust Plugin). |
| **Polyglot Memory Fusion** | `cross_lang_memory_fusion/` | Proves true language agnosticism. Physically shares complex data structures across entirely different languages (C, Rust, C++, Zig) at bare-metal speed with absolutely zero JSON serialization. |

---

## Running the Tests

To compile and run any of these tests, use the ORE CLI:

```bash
# Standard Tools
ore mktool script.rs

# Host Agents (that load plugins)
ore mktool host_agent.rs --host

# Dynamic Plugins (Memory Fusion)
ore mktool plugin.rs --shared
```
