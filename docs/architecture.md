# Architecture

> How ORE is built, and why every layer exists.

## Overview

ORE is a **kernel-level process manager** for local AI. It sits between user-facing applications and raw inference hardware, providing security, multi-tenant scheduling, memory management, zero-trust sandboxing, and inter-process communication.

Applications never talk to the GPU or host operating system directly. They talk to ORE. ORE enforces the rules.

```text
╔═══════════════════════╗     ╔═══════════════════════╗
║      User App A       ║     ║      User App B       ║
║   (e.g. OpenClaw)     ║     ║  (e.g. Custom Agent)  ║
╚══════════╤════════════╝     ╚════════════╤══════════╝
           │  REST / IPC / Execute         │  REST / IPC / Execute
           └──────────────┬────────────────┘
                          ▼
╔══════════════════════════════════════════════════════╗
║                  ORE KERNEL  (Rust)                  ║
║                                                      ║
║   ┌─────────────┐    ┌──────────────────────────┐    ║
║   │ Auth Guard  │───▶│ Manifest Permission Check│    ║
║   │(Bearer JWT) │    │   + Rate Limiter         │    ║
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
║   │  · Table Hijacking (__indirect_function)     │   ║
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
║   │  · Message Bus  (mpsc non-blocking queues)   │   ║
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

### 1. Inference Lifecycle (`/ask`, `/run`)

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
│    Enforce token rate limit         │
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
                                       Capture stdout/stderr & return
```

---

## Workspace Layout

```text
ore-system/
├── ore-core/                Kernel logic
│   ├── driver.rs            HAL trait (InferenceDriver) + shared types
│   ├── firewall.rs          Context firewall (PII redaction, injection heuristics)
│   ├── ipc.rs               MessageBus, SemanticBus (w/ cache + GC), RateLimiter
│   ├── scheduler.rs         Multi-tenant GpuScheduler, ModelRegistry, MemoryAccountant
│   ├── memory.rs            Memory Management (context persistence & KV-cache paging)
│   ├── registry.rs          App manifest registry (TOML loader, validation)
│   ├── sandbox.rs           Zero-Trust WASM Sandbox, VFS, network portal watcher
│   ├── crypto.rs            VFS-mapped hardware cryptographic subsystem
│   ├── linker/              WebAssembly Dynamic Linker (ore-ld)
│   │   ├── mod.rs           Linker entrypoint
│   │   ├── linker_state.rs  Handle tracking & module registry
│   │   ├── mmu.rs           MMU: memory.grow allocation, -fPIC C-ABI globals
│   │   └── syscalls.rs      Table expansion & plugin loading (ore_dlopen, ore_dlsym)
│   └── inference/           Inference Engine Implementations
│       ├── external/        External inference drivers
│       │   └── ollama.rs    OllamaDriver (HTTP proxy to Ollama daemon)
│       └── native/          Native Candle Engine
│           ├── mod.rs       NativeDriver (GGUF loading + hardware detection)
│           ├── engine.rs    OreEngine enum (Llama/Qwen2/Qwen3 MoE) + ActiveEngine
│           ├── gguf_tokenizer.rs GGUF metadata tokenizer extractor
│           └── models/      Architecture-specific model loaders
│               ├── llama.rs Llama family loader
│               ├── qwen2.rs Qwen2 family loader
│               ├── qwen3.rs Qwen3 MoE family loader
│               ├── bert.rs  BERT embedder (all-MiniLM)
│               └── nomic.rs Nomic v1.5 embedder (Safetensors)
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
│       ├── inference.rs     ask_ai (secured + paged), run_process (streamed)
│       └── ipc.rs           Semantic bus share/search, agent messaging
├── ore-cli/                 Interactive CLI tool (clap + dialoguer)
│   ├── main.rs              Command dispatch + package downloads
│   ├── cli.rs               Clap argument definitions
│   ├── interactive.rs       Interactive wizards (init, manifest)
│   ├── utils.rs             HTTP client helpers, token file reader
│   └── src/syskit/          C and Zig SDK headers (ore.h, ore.zig) for plugins
├── ore-sys/                 Rust SDK Crate for building WASM plugins
│   └── src/                 SDK Source Code (`ore_bind!`, `ore_export!`, `Plugin`)
├── manifests/               App permission manifests (.toml files)
├── models/                  Downloaded model weights
├── memory/                  SSD page files for agent context and KV-caches
├── runtimes/                Language runtimes for Inception Mode (system-py, system-js)
├── tools/                   Pre-compiled WebAssembly tools (Console Cartridges)
├── tests/                   Integration tests (memory_fusion, linker, sandbox)
├── ore.toml                 System configuration
├── Cargo.toml               Workspace configuration + release profile
└── rust-toolchain.toml      Pinned Rust 1.93.0
```

---

## Design Principles

1. **Zero-Trust Security by Default** - Every prompt is firewalled. Every route is authenticated. Every tool executes inside a sandboxed WASM container with capability-restricted file and network permissions.
2. **Multi-Tenant VRAM Bin-Packing** - Instead of single-model locks, the GPU scheduler manages physical VRAM budgets via NVML hardware monitoring, dynamically dimensions KV caches from GGUF geometry, and gracefully evicts LRU idle models when memory pressure arises.
3. **Zero-RAM AOT Caching (`.cwasm`)** - WASM cartridges are compiled to native machine code once and serialized. Subsequent invocations dynamically `mmap` the pre-compiled binary directly from the OS Page Cache into the CPU, yielding ~10ms cold starts with zero memory bloat.
4. **Zero-Copy Cross-Language Memory Fusion (`ore-ld`)** - A custom POSIX-compliant dynamic linker allows host tools written in Rust, C, C++, or Zig to load position-independent plugins (`.wasi.so`) directly into shared linear memory, bypassing serialization overhead.
5. **Layer 7 Network Isolation & Runtime Shims** - Pure compute sandboxes lack raw sockets. ORE routes egress HTTP requests through an asynchronous Layer 7 proxy with manifest-enforced domain, method, and path rules, supported by CommonJS and Python WASI shims.
6. **OS-Style Memory Management** - Idle agent conversational context and physical KV caches are serialized to the SSD (`memory/` directory) and restored on demand. Background memory compaction keeps context sizes bounded.
7. **Driver Abstraction (HAL)** - The `InferenceDriver` trait provides an engine-agnostic boundary with 11 core methods, enabling transparent swapping between Native Candle and Ollama backends.

---

**Next:** [Getting Started →](./getting-started.md)
