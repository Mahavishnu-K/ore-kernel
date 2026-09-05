# Polyglot Agent Tooling Projects (`project_tools`)

## Purpose
This directory contains a suite of **Complex Polyglot Cartridge Projects**. While other test folders validate single-script compilation and sandbox security, this directory proves that ORE's compilation toolchain (`ore mktool`) can seamlessly handle full-fledged projects with their own dependency trees, package managers, and multi-file architectures, compiling them down into single standalone WebAssembly Nano-Services.

## Architecture & Existence
When AI agents require advanced tooling, they often need complex libraries (e.g., Zod for validation, Serde for serialization). This folder demonstrates how ORE supports these standard developer ecosystems:

| Ecosystem | Directory | Description |
|---|---|---|
| **Go** | `go/` | A standard Go module (`go.mod`) with external dependencies. Demonstrates Go's compilation toolchain targeting WASI for ORE. |
| **Node.js** | `javascript/` | A standard Node.js-style project (`package.json`) with modular imports. ORE bundles JS and NPM dependencies, executing via `Javy` WASM runtime. |
| **TypeScript** | `typescript/` | A TS project importing external NPM libraries (e.g., `zod`). Showcases ORE's ability to bundle, transpile, and execute strict TS logic as a Nano-Service. |
| **Rust** | `rust/` | A standard Cargo workspace (`Cargo.toml`) utilizing external crates (`serde`). Acts as a high-performance native agent tool. |
| **Python** | `python/` | A multi-file Python project utilizing `requirements.txt`. Demonstrates ORE bundling a structured Python workspace into a portable WASI Cartridge. |

## How it works with ORE
Instead of building bloated Docker containers with `npm install` or `go mod download` just to give an AI agent access to a tool, developers can run `ore mktool` on these projects. ORE resolves the dependencies natively and outputs a universally portable `.wasm` cartridge. This cartridge can then be safely executed in less than 50 microseconds by any Agent running on the ORE Kernel, completely independent of the host OS or installed runtimes.
