# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Network Capabilities & Security**: Enhanced network capabilities in WASM agent. Added support for additional network rules in `wasm_agent.toml`, improved metadata handling in sandbox for HTTP requests (including status, headers, and cookies), implemented real-time streaming and response handling in `ORE_Response` class, and added comprehensive network upgrade tests for both Python and JavaScript in `run_network_upgrades.py`. Updated network handling and security.
- **Runtime Shims**: Added CommonJS and Python shims for module compatibility.
- **VRAM Bin-Packing (Multi-Tenancy)**: The `GpuScheduler` was fully re-architected to support multiple models co-residing in VRAM. Replaced the single-model `Semaphore(1)` with a `ModelRegistry`, `MemoryAccountant`, and `NvmlGpuMemoryProvider` to enable physical VRAM tracking, dynamic KV cache estimation, and LRU eviction.
- **Polyglot Memory Fusion (`ore-ld`)**: A `dlopen`-style WebAssembly dynamic linker allowing AI agents to physically share linear memory space with loaded plugins (`.wasi.so`) for zero-copy, cross-language data mutation.
- `ore mktool` compiler flags for Memory Fusion: `--shared` (to compile dynamic plugins) and `--host` (to compile host tools that load plugins).
- `tests/cross_lang_memory_fusion/` test suite proving zero-copy data sharing between C->Rust, Rust->C++, and Zig->C.
- Comprehensive SDK documentation for building plugins across Rust, C/C++, and Zig (`docs/building-plugins.md`).
- **Python Execution Support:** Added capability to execute both single-file Python scripts (`simple_script.py`) and full Python project workspaces natively within the Ore WebAssembly sandbox using the pre-compiled Python WASI runtime.
- Updated `.gitignore` to include `/memory/` and `/bin/` directories for cleaner builds.
- Initial setup for the `ore-kernel` open-source repository.
- Community standards: `CODE_OF_CONDUCT.md`.
- Security policy: `SECURITY.md`.
- Universal Polyglot Toolchain and cross-language orchestration.
- **AOT (Ahead-Of-Time) Zero-RAM Caching (`.cwasm`)**: WebAssembly modules are now JIT compiled once and serialized to disk. Subsequent runs instantly `mmap` the native machine code from the OS Page Cache directly to the CPU, reducing boot latency from ~600ms to 10ms for massive polyglot swarms.
- **Node.js Polyfills via QuickJS**: Standard JS/TS modules are now natively mounted into `/modules` (Read-Only) automatically during JS sandbox execution.
- **Dynamic Filesystem Cache Invalidation**: The Ore Kernel now utilizes raw `std::fs::metadata` timestamps to surgically track `.wasm` updates and overwrite stale `.cwasm` AOT caches automatically, with absolutely zero `DashMap` or in-memory bloat.
- **Agentic Inception Upgrades**: The Sandbox natively accommodates on-the-fly script materialization for self-improving AI models directly into the Virtual File System.
- **`ore-common` Workspace Crate**: Introduced a dedicated lightweight shared foundation library across `ore-core`, `ore-server`, and `ore-cli`. Centralizes protocol DTOs (`protocol.rs`), typed AppManifest schemas and validation (`manifest.rs`), system configurations (`config.rs`), canonical paths (`paths.rs`), global constants (`constants.rs`), and ANSI text length utilities (`text.rs`), completely eliminating code duplication and compile-time drift with zero runtime bloat.
- **Modular Sandbox Subsystem (`ore-core/src/sandbox/`)**: Deconstructed the 1,000+ line monolithic `sandbox.rs` into a clean, production-grade modular subsystem:
  - `params.rs`: Data contracts (`ExecuteParams`, `OreResponseMeta`).
  - `state.rs`: `OreSandboxState` and RAII Drop guards (`TempDirGuard` for ephemeral cleanup, `ThreadJoinGuard` for atomic portal thread termination).
  - `cache.rs`: Ahead-Of-Time (`.cwasm`) compiler, timestamp validator, and zero-RAM `mmap` loader.
  - `vfs.rs`: Capability-based directory mounting, UNC prefix sanitization, and QuickJS standard library polyfill mounts.
  - `shims.rs`: Syscall host function bindings (`ore::fetch`), WasmEdge dynamic shadowing (`sock_accept`), and indirect OS routing table initialization.
  - `portals/`: Decoupled background IPC daemons for hardware crypto (`crypto.rs`) and concurrent Layer 7 network watching (`network.rs`).
  - `net/`: Standalone Layer 7 egress proxy engine (`fetch.rs`).
- **JavaScript/TypeScript Execution Safeguard**: Added `wrap_tla_with_error_boundary` in `ore-server/src/execution/script.rs` to automatically enclose autonomous script bodies inside Top-Level Await promise handlers with clean unhandled rejection routing to STDERR.
### Changed
- **Workspace Architecture**: Expanded the Rust workspace to 5 member crates (`ore-common`, `ore-core`, `ore-server`, `ore-cli`, `ore-sys`).
- **Canonical System Paths & Constants**: Migrated path resolution (`get_ore_dir`, `get_models_dir`, `get_manifests_dir`, `get_token_path`, `get_memory_dir`, `get_runtimes_dir`, `get_plugins_dir`) and ports/fuel limits across `ore-core`, `ore-server`, and `ore-cli` to use `ore_common::paths` and `ore_common::constants`.
- **Execution Modules**: Refactored `execution` as a module for handling various execution modes.
- **Configuration & Utilities**: Simplified configuration file retrieval in utility functions.
- **Error Handling**: Improved error handling and formatting in WasmSandbox network checks.
- **Code Quality**: Cleaned up code formatting and improved readability across multiple files.
- **Sandbox Architecture**: Replaced monolithic `sandbox.rs` with the modular `ore-core/src/sandbox/` subsystem.
- Enhanced `main.rs` to dynamically detect Python projects and manage their dependencies via `requirements.txt`.
- Upgraded `sandbox/vfs.rs` to set the `PYTHONPATH` environment variable, enabling proper module resolution for the Python WASI runtime.
- Improved `system.rs` to securely read and pass tool arguments from a dedicated `.args` file.
- Modified `test_agent.toml` to officially allow Python as an executable tool.
- Refactored path display logic during cartridge compilation in `ore-cli`.
- Updated string formatting in `ore-server` IPC and system handlers.

### Deprecated
- None yet.

### Removed
- `DashMap` and manual `cache_key` tracking from the Sandbox AOT Cache. Tool cache state is now entirely managed via OS File Metadata, maximizing performance and concurrency without RAM bloat.

### Fixed
- Clean up of temporary `.tmp_build` directories natively post-execution.

### Security
- None yet.

## [0.1.0] - 2026-07-22

### Added
- Initial project structure containing `ore-core`, `ore-server`, and `ore-cli`.
- Documentation in the `docs/` directory.

[Unreleased]: https://github.com/Mahavishnu-K/ore-kernel/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Mahavishnu-K/ore-kernel/releases/tag/v0.1.0
