# Custom Language Agent Tooling & Security Tests (`wasm_sandbox_tools`)

## Purpose
This directory contains a suite of test scripts written in various programming languages (Rust, Go, TypeScript, JavaScript). Its primary purpose is to validate the compilation of custom language agent tooling into WebAssembly Nano-Services, and to rigorously test the security boundaries of the ORE Zero-Trust Sandbox.

## Architecture & Existence
The ORE ecosystem replaces Docker containers by executing agent tools in an in-process WebAssembly sandbox with extreme security restrictions. The files in this folder are used to verify those protections are working.

Key test scripts include:

| Test Script | Language | Security Boundary / Purpose |
|---|---|---|
| **`network_interception.rs`** | Rust | Tests the Layer 7 network proxy. Verifies the firewall successfully blocks unauthorized egress connections. |
| **`vfs_security_check.go`** | Go | Comprehensive check on the Virtual File System (VFS). Verifies path traversals and unauthorized file writes are blocked by the OS-level WASI integration. |
| **`infinite_loop_trap.rs`** | Rust | Validates the deterministic CPU Fuel limit. Spins up an infinite loop to ensure the sandbox safely traps and panics without hanging the host CPU. |
| **`fibonacci_benchmark.ts`** & **`rust_cruncher.rs`** | TS / Rust | Benchmarking tools to test pure compute performance within the sandbox. |
| **`js_formatter.js`** & **`ts_generator.ts`** | JS / TS | Demonstrates ORE's capability to safely compile and execute interpreted JS/TS within the WASM sandbox environment without a heavy node.js installation. |
| **`go_hasher.go`** | Go | A Go-based hashing benchmark to validate compute performance and standard library capability within the sandbox. |
| **`simple_script.py`** | Python | Validates ORE's execution of basic Python scripts inside the sandbox, leveraging the pre-compiled Python WASI runtime. |

## How it works with ORE
These files are targeted by the `ore mktool` CLI compiler. Developers and testers compile these files into portable `.wasm` cartridges to ensure that no matter what language a tool is written in, the ORE Kernel applies the same strict security guarantees on Networking, Memory, and CPU Execution.
