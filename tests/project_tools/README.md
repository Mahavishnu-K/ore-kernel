# Polyglot Agent Tooling Projects (`project_tools`)

## Purpose
This directory contains a suite of **Complex Polyglot Cartridge Projects**. While other test folders validate single-script compilation and sandbox security, this directory proves that ORE's compilation toolchain (`ore mktool`) can seamlessly handle full-fledged projects with their own dependency trees, package managers, and multi-file architectures, compiling them down into single standalone WebAssembly Nano-Services.

## Architecture & Existence
When AI agents require advanced tooling, they often need complex libraries (e.g., Zod for validation, Serde for serialization). This folder demonstrates how ORE supports these standard developer ecosystems:

- **`go/`**: A standard Go module (`go.mod`) containing multiple files and external dependencies (like JSON parsers). It demonstrates how Go's compilation toolchain targets WASI for ORE.
- **`javascript/`**: A standard Node.js-style project (`package.json`) with modular imports. It demonstrates how ORE bundles JavaScript and its NPM dependencies, executing them using the `Javy` WASM runtime for high-performance I/O.
- **`typescript/`**: A TypeScript project that imports external NPM libraries (e.g., `zod` for strict schema validation). It showcases ORE's ability to bundle, transpile, and execute strict TypeScript logic as a Nano-Service.
- **`rust/`**: A standard Cargo workspace (`Cargo.toml`) utilizing external crates like `serde` and `serde_json`. It reads JSON payloads from STDIN, processes them, and outputs to STDOUT, acting as a high-performance native agent tool.

## How it works with ORE
Instead of building bloated Docker containers with `npm install` or `go mod download` just to give an AI agent access to a tool, developers can run `ore mktool` on these projects. ORE resolves the dependencies natively and outputs a universally portable `.wasm` cartridge. This cartridge can then be safely executed in less than 50 microseconds by any Agent running on the ORE Kernel, completely independent of the host OS or installed runtimes.
