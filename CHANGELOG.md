# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Polyglot Memory Fusion (`ore-ld`)**: A custom POSIX-compliant WebAssembly dynamic linker allowing AI agents to physically share linear memory space with loaded plugins (`.wasi.so`) for zero-copy, cross-language data mutation.
- `ore mktool` compiler flags for Memory Fusion: `--shared` (to compile dynamic plugins) and `--host` (to compile host tools that load plugins).
- `tests/cross_lang_memory_fusion/` test suite proving zero-copy data sharing between C->Rust, Rust->C++, and Zig->C.
- Comprehensive SDK documentation for building plugins across Rust, C/C++, and Zig (`docs/building-plugins.md`).
- Initial setup for the `ore-kernel` open-source repository.
- Community standards: `CODE_OF_CONDUCT.md`.
- Security policy: `SECURITY.md`.
- Universal Polyglot Toolchain and cross-language Nano-Service orchestration.

### Changed
- Refactored path display logic during cartridge compilation in `ore-cli`.
- Updated string formatting in `ore-server` IPC and system handlers.

### Deprecated
- None yet.

### Removed
- None yet.

### Fixed
- None yet.

### Security
- None yet.

## [0.1.0] - 2026-07-22

### Added
- Initial project structure containing `ore-core`, `ore-server`, and `ore-cli`.
- Documentation in the `docs/` directory.

[Unreleased]: https://github.com/Mahavishnu-K/ore-kernel/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Mahavishnu-K/ore-kernel/releases/tag/v0.1.0
