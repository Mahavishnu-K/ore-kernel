# Architecture

> How Ore is built, and why every layer exists.

## Overview

Ore is a **kernel-level process manager** for local AI. It sits between user-facing applications and raw inference hardware, providing security, multi-tenant scheduling, memory management, zero-trust sandboxing, and inter-process communication.

Applications never talk to the GPU or host operating system directly. They talk to Ore. Ore enforces the rules.

```text
╔═══════════════════════╗     ╔═══════════════════════╗
║      User App A       ║     ║      User App B       ║
║   (e.g. OpenClaw)     ║     ║  (e.g. Custom Agent)  ║
╚══════════╤════════════╝     ╚════════════╤══════════╝
           │  REST / IPC / Execute         │  REST / IPC / Execute
           └──────────────┬────────────────┘
                          ▼
╔══════════════════════════════════════════════════════╗
║                  Ore KERNEL  (Rust)                  ║
║                                                      ║
║   ┌─────────────┐    ┌──────────────────────────┐    ║
║   │ Auth Guard  │───▶│ Manifest Permission Check│    ║
║   │(Bearer UUID)│    │   + Rate Limiter         │    ║
║   └─────────────┘    └────────────┬─────────────┘    ║
║                                   │                  ║
║   ┌─────────────────┐             │                  ║
║   │ Context Firewall│◀────────────┘                  ║
║   │  · Inj. Detect  │                                ║
║   │  · PII Redact   │                                ║
║   └────────┬────────┘                                ║
║            │                                         ║
║   ┌────────▼──────────────────────────────────────┐  ║
║   │  Multi-Tenant GPU Scheduler                   │  ║
║   │  · ModelRegistry & Physical MemoryAccountant  │  ║
║   │  · Dynamic KV-Cache Estimation (GGUF geometry)│  ║
║   │  · LRU Model Eviction & RAII GpuLease (32 max)│  ║
║   └───────────────────────────────────────────────┘  ║
║                                                      ║
║   ┌──────────────────────────────────────────────┐   ║
║   │  Modular Execution Engine & WASM Sandbox     │   ║
║   │  · Tool Mode: Pre-compiled Cartridges (.wasm)│   ║
║   │  · Script Mode: Inception (Py/JS + VFS shims)│   ║
║   │  · Shell Mode: Host execution (UNSAFE audit) │   ║
║   │  · AOT Zero-RAM Caching (.cwasm via mmap)    │   ║
║   │  · Layer 7 Egress Network & Crypto Portals   │   ║
║   └──────────────────────────────────────────────┘   ║
║                                                      ║
║   ┌──────────────────────────────────────────────┐   ║
║   │  Dynamic Linker (ore-ld) & Memory Fusion     │   ║
║   │  · ore_dlopen / dlsym / dlclose / dlerror    │   ║
║   │  · Table slots + 1 MiB recycled memory slabs │   ║
║   │  · Linear Memory Sharing (Zero-Copy FFI)     │   ║
║   └──────────────────────────────────────────────┘   ║
║                                                      ║
║   ┌──────────────────────────────────────────────┐   ║
║   │  Memory Management  (Agent Context Swap)     │   ║
║   │  · Page Out/In (RAM ↔ SSD JSON Freeze)       │   ║
║   │  · Page Out/In (KV-Cache .safetensors)       │   ║
║   │  · Background Compaction & Auto-Summarize    │   ║
║   └──────────────────────────────────────────────┘   ║
║                                                      ║
║   ┌──────────────────────────────────────────────┐   ║
║   │  IPC Layer                                   │   ║
║   │  · Message Bus  (bounded mpsc, 1024/agent)   │   ║
║   │  · Semantic Bus (Vector memory + dot product)│   ║
║   │  · Zero-Copy Embedding Cache (DashMap + Arc) │   ║
║   │  · Memory GC  (Hourly TTL-based sweep)       │   ║
║   │  · Semantic Persistence (SSD Bincode Pipes)  │   ║
║   └──────────────────────────────────────────────┘   ║
╚══════════════════════════╤═══════════════════════════╝
                           │
                           ▼
╔══════════════════════════════════════════════════════╗
║             HARDWARE ABSTRACTION LAYER               ║
║     ┌───────────────┐    ┌───────────────────┐       ║
║     │ Native Candle │    │  Ollama API Proxy │       ║
║     │(GGUF · CPU/GPU│    │  (HTTP · Streaming│       ║
║     │ CUDA · Metal) │    │   · Embeddings)   │       ║
║     └───────┬───────┘    └───────────────────┘       ║
║             │                                        ║
║     ┌───────▼───────┐                                ║
║     │Native Embedder│                                ║
║     │ (Safetensors: │                                ║
║     │  BERT / Nomic)│                                ║
║     │ (Zero-RAM     │                                ║
║     │  Idle Design) │                                ║
║     └───────────────┘                                ║
╚══════════════════════════╤═══════════════════════════╝
                           │
                           ▼
                  ┌──────────────────┐
                  │  GPU / NPU / CPU │
                  └──────────────────┘
```

---

## Request Lifecycles

### 1. Inference Lifecycle (`/infer`, `/run`)

```text
Client (curl / CLI / App)
  │
  ▼
┌─────────────────────────────────────┐
│ 1. AUTH MIDDLEWARE                   │  ore-server/src/middleware.rs
│    Extract Authorization header     │
│    Compare Bearer token             │
│    Reject 401 if invalid            │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 2. ROUTE HANDLER                    │  ore-server/src/handlers/inference.rs
│    Parse request (model + prompt)   │
│    Lookup AppManifest from registry │
│    Enforce rate limit (/run)        │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 3. CONTEXT FIREWALL                 │  ore-core/src/firewall.rs
│    InjectionBlocker::check()        │
│    PiiRedactor::redact()            │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 4. MEMORY MANAGER (if stateful)     │  ore-core/src/memory.rs
│    Pager::page_in_history()         │
│    Pager::page_in_kv_cache()        │
│    Append new message to context    │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 5. GPU SCHEDULER                    │  ore-core/src/scheduler.rs
│    Acquire permit (out of 32)       │
│    Compute KV-cache from GGUF math  │
│    MemoryAccountant admission check │
│    LRU model eviction if VRAM full  │
│    Return GpuLease (RAII)           │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 6. INFERENCE DRIVER (HAL)           │  ore-core/src/driver.rs
│    driver.generate_text()           │
│    Stream tokens via mpsc channel   │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 7. RESPONSE + CLEANUP               │
│    Stream tokens to client          │
│    Check memory limits & Summarize  │
│    Pager::page_out_history/kv_cache │
│    GpuLease drops → VRAM released   │
└─────────────────────────────────────┘
```

### 2. Sandbox Execution Lifecycle (`/execute`)

```text
Client (WASM Tool, Inception Script, or Shell Request)
  │
  ▼
┌─────────────────────────────────────┐
│ 1. AUTH & REGISTRY DISPATCH          │  ore-server/src/execution/mod.rs
│    Validate Bearer token            │
│    Inspect AppManifest permissions  │
│    Resolve ExecutionMode (Tool,     │
│       Script, or Shell)             │
└──────────────┬──────────────────────┘
               │
       ┌───────┼──────────────────────────────┐
       ▼                                      ▼
[Shell Mode]                           [Tool / Script Mode]
Check can_execute_shell                Check can_execute_wasm
Execute on host OS (UNSAFE)            Resolve AOT .cwasm or compile
Return stdout/stderr                   Mount VFS (/workspace, /ore_tmp)
                                       Inject Shims (CommonJS / Python)
                                       Spawn blocking thread in WasmSandbox
                                       Monitor Layer 7 Network Portal
                                       Enforce max_cpu_instructions fuel
                                       Capture stdout/stderr (10 MiB each)
                                       Classify exit / trap by error type
                                       Return output (+ KERNEL ERROR line)
```

---

## Workspace Layout

```text
ore-system/
├── ore-common/              Shared foundation crate (zero runtime bloat)
│   ├── constants.rs         Centralized ports (6767, 11434), fuel limits, default paths
│   ├── paths.rs             Canonical path resolvers (~/.ore, models, memory, manifests)
│   ├── protocol.rs          Universal protocol DTOs & API schemas across Server, CLI, Core
│   ├── manifest.rs          Typed AppManifest schema and validation engine
│   ├── config.rs            System configuration schemas (OreConfig, SystemConfig, MemoryConfig)
│   ├── types.rs             Domain types (LocalModel, VramProcess, ContextMessage)
│   └── text.rs              ANSI-aware terminal text measuring and truncation utilities
├── ore-core/                Kernel logic
│   ├── driver.rs            HAL trait (InferenceDriver) + shared types
│   ├── firewall.rs          Context firewall (PII redaction, injection heuristics)
│   ├── ipc.rs               MessageBus, SemanticBus (w/ cache + GC), RateLimiter
│   ├── scheduler.rs         Multi-tenant GpuScheduler, ModelRegistry, MemoryAccountant
│   ├── memory.rs            Memory Management (context persistence & KV-cache paging)
│   ├── registry.rs          App manifest registry (TOML loader, validation)
│   ├── sandbox/             Modular Zero-Trust WASM Sandbox Subsystem
│   │   ├── mod.rs           Hypervisor facade (WasmSandbox) & execution pipeline
│   │   ├── params.rs        Data contracts (ExecuteParams, OreResponseMeta)
│   │   ├── state.rs         Master sandbox state & RAII Drop guards (TempDirGuard, ThreadJoinGuard)
│   │   ├── cache.rs         AOT (.cwasm) compiler, metadata timestamp validator & mmap loader
│   │   ├── vfs.rs           WASI capability directory mounting & UNC path sanitization
│   │   ├── shims.rs         Host syscalls (ore::fetch), WasmEdge sock_accept shadowing & OS table
│   │   ├── portals/         Concurrent VFS IPC daemons (crypto.rs, network.rs)
│   │   └── net/             Layer 7 Egress Network Engine (fetch.rs)
│   ├── crypto.rs            VFS-mapped hardware cryptographic subsystem
│   ├── linker/              WebAssembly Dynamic Linker (ore-ld)
│   │   ├── mod.rs           Linker entrypoint
│   │   ├── linker_state.rs  Loaded plugins, handles, last error, free memory slabs
│   │   ├── mmu.rs           MMU: memory.grow allocation, -fPIC C-ABI globals
│   │   └── syscalls.rs      ore_dlopen / ore_dlsym / ore_dlclose / ore_dlerror
│   └── inference/           Inference Engine Implementations
│       ├── external/        External inference drivers
│       │   └── ollama.rs    OllamaDriver (HTTP proxy to Ollama daemon)
│       └── native/          Native Candle Engine
│           ├── mod.rs       NativeDriver (hardware detection, generation, token counting)
│           ├── engine.rs    OreEngine enum (Llama / Qwen2 / Qwen3 / Qwen3-MoE) + architecture router
│           ├── kv_manager.rs KV-cache flatten/unflatten for .safetensors paging
│           ├── gguf_tokenizer.rs GGUF metadata tokenizer extractor
│           └── models/      Architecture-specific model loaders
│               ├── llama.rs     Llama family
│               ├── qwen2.rs     Qwen2 family
│               ├── qwen3.rs     Qwen3 (dense)
│               ├── qwen3_moe.rs Qwen3-MoE (also loads qwen2moe)
│               ├── bert.rs      BERT embedder (all-MiniLM)
│               ├── nomic.rs     Nomic v1.5 embedder (Safetensors)
│               ├── transformers/ Quantized layers, fused MoE
│               └── nn/          KV cache
├── ore-server/              Axum HTTP daemon
│   ├── main.rs              Boot sequence, router, token generator, GC scheduler
│   ├── state.rs             KernelState + OreConfig (shared application state)
│   ├── middleware.rs        Bearer token authentication middleware
│   ├── payloads.rs          Request payload schemas and mode resolvers
│   ├── execution/           Modular execution engine
│   │   ├── mod.rs           Execution coordinator and parameter builder
│   │   ├── tool.rs          Fixed Tool Mode (.wasm cartridge resolution)
│   │   ├── script.rs        Autonomous Script Mode (Inception, dependency mounts)
│   │   └── shell.rs         Host Shell Execution bypass handler
│   ├── shims/               Runtime polyfills for WASM
│   │   ├── javascript/      Node.js globals and CommonJS require() bridge
│   │   └── python/          WASI asyncio event loop, requests/httpx shims
│   └── handlers/            Route handlers
│       ├── system.rs        Health, ps, ls, agents, manifests, pull, load, expel
│       ├── inference.rs     run_agent_inference (secured + paged), run_user_inference (streamed)
│       └── ipc.rs           Semantic bus share/search, agent messaging
├── ore-cli/                 Interactive CLI tool (clap + dialoguer)
│   ├── main.rs              Command dispatch + package downloads
│   ├── cli.rs               Clap argument definitions
│   ├── interactive.rs       Interactive wizards (init, manifest)
│   ├── telemetry.rs         Dashboards for status, top, ps, ls
│   ├── utils.rs             HTTP client helpers, token file reader
│   └── syskit/              C and Zig SDK headers (ore.h, ore.zig) for ore-ld
├── ore-sys/                 Rust SDK for ore-ld hosts and plugins
│   ├── host.rs              `Plugin` (load, get_func, close, last_error, Drop) + `ore_bind!` (feature "host")
│   └── plugin.rs            `ore_export!` + no_std panic handler (feature "plugin")
├── manifests/               App permission manifests (.toml files)
├── models/                  Downloaded model weights
├── memory/                  SSD page files for agent context and KV-caches
├── runtimes/                Language runtimes for Inception Mode (system-py, system-js)
├── tools/                   Pre-compiled WebAssembly tools (Console Cartridges)
├── plugins/                 ore-ld plugins (.wasi.so)
├── tests/                   Tool sources for integration tests: memory_fusion, cross_lang_memory_fusion,
│                            linker_test, wasm_sandbox_tools, project_tools
├── examples/                Python agents and test runners that drive the kernel over HTTP
├── ore.toml                 System configuration
├── Cargo.toml               Workspace configuration + release profile
└── rust-toolchain.toml      Pinned Rust 1.93.0
```

> **Directory resolution:** data directories (`manifests/`, `models/`, `memory/`, `runtimes/`, `tools/`, `plugins/`) and `ore.toml` are resolved relative to the **Ore base directory**, found in this order: 1) the `$ORE_DIR` environment variable, 2) `./` if it contains `ore.toml`, 3) `../` if it contains `ore.toml` (both for local development), 4) `~/.ore` on Linux/macOS or `%USERPROFILE%\.ore` on Windows. The auto-installed Zig lives in `toolchains/` under the same directory.

### Key Dependencies

| Crate | Purpose |
|---|---|
| `axum` | HTTP server and auth middleware |
| `tokio` + `tokio-stream` | Async runtime, semaphore scheduling, response streaming |
| `candle-core` + `candle-transformers` + `candle-nn` | Native GGUF inference and safetensors embedders |
| `wasmtime` + `wasmtime-wasi` | WebAssembly sandbox, fuel metering, WASI file system |
| `tokenizers` | Hugging Face tokenizers |
| `dashmap` | Concurrent maps for the IPC buses, rate limiter, and embedding cache |
| `nvml-wrapper` + `sysinfo` | VRAM and system memory readings for the scheduler |
| `clap` + `dialoguer` | CLI parsing and the interactive wizards |
| `reqwest` + `hf-hub` | Ollama driver and Hugging Face model downloads |
| `regex` | Firewall PII and injection patterns |
| `serde` + `toml` | Manifest and config parsing |
| `thiserror` + `anyhow` | Error types |

---

## Design Principles

1. **Zero-Trust Security by Default** - Every route is authenticated, every inference prompt passes the firewall heuristics, and every tool runs inside a WASM sandbox with capability-restricted file and network access. Failures are classified by type, so a crash can't masquerade as success.
2. **Multi-Tenant VRAM Bin-Packing** - Instead of single-model locks, the GPU scheduler manages physical VRAM budgets via NVML hardware monitoring, dynamically dimensions KV caches from GGUF geometry, and gracefully evicts LRU idle models when memory pressure arises.
3. **Zero-RAM AOT Caching (`.cwasm`)** - WASM cartridges are compiled to native machine code once and serialized. Subsequent invocations dynamically `mmap` the pre-compiled binary directly from the OS Page Cache into the CPU, yielding ~10ms cold starts with zero memory bloat.
4. **Zero-Copy Cross-Language Memory Fusion (`ore-ld`)** - A `dlopen`-style dynamic linker for WebAssembly lets host tools written in Rust, C, C++, or Zig load position-independent plugins (`.wasi.so`) directly into shared linear memory, bypassing serialization overhead.
5. **Layer 7 Network Isolation & Runtime Shims** - Pure compute sandboxes lack raw sockets. Ore routes egress HTTP requests through a Layer 7 proxy with manifest-enforced domain, method, and path rules (re-checked on every redirect hop), supported by CommonJS and Python WASI shims.
6. **OS-Style Memory Management** - Idle agent conversational context and physical KV caches are serialized to the SSD (`memory/` directory) and restored on demand. Background memory compaction keeps context sizes bounded.
7. **Driver Abstraction (HAL)** - The `InferenceDriver` trait provides an engine-agnostic boundary with 13 methods (11 required), enabling transparent swapping between Native Candle and Ollama backends.

---

**Next:** [Getting Started →](./getting-started.md)
