# Architecture

> How ORE is built, and why every layer exists.

## Overview

ORE is a **kernel-level process manager** for local AI. It sits between user-facing applications and raw inference hardware, providing security, scheduling, memory management, and inter-process communication.

Applications never talk to the GPU directly. They talk to ORE. ORE enforces the rules.

```
╔═══════════════════════╗     ╔═══════════════════════╗
║      User App A       ║     ║      User App B       ║
║   (e.g. OpenClaw)     ║     ║  (e.g. Custom Agent)  ║
╚══════════╤════════════╝     ╚════════════╤══════════╝
           │  REST / IPC                   │  REST / IPC
           └──────────────┬────────────────┘
                          ▼
╔══════════════════════════════════════════════════════╗
║                  ORE KERNEL  (Rust)                  ║
║                                                      ║
║   ┌─────────────┐    ┌──────────────────────────┐    ║
║   │ Auth Guard  │───▶│ Manifest Permission Check│   ║
║   │(Bearer JWT) │    │   + Rate Limiter          │   ║
║   └─────────────┘    └────────────┬─────────────┘    ║
║                                   │                  ║
║   ┌─────────────────┐             │                  ║
║   │ Context Firewall│◀────────────┘                  ║
║   │  · Inj. Detect  │                                ║
║   │  · PII Redact   │                                ║
║   │  · Boundary Tag │                                ║
║   └────────┬────────┘                                ║
║            │                                         ║
║   ┌────────▼──────────────────────────────────────┐  ║
║   │  Priority Scheduler  ──▶  GPU Semaphore Lock  │  ║
║   └───────────────────────────────────────────────┘  ║
║                                                      ║
║   ┌──────────────────────────────────────────────┐   ║
║   │  SSD Pager  (Agent Context Swap)             │   ║
║   │  · Page Out (RAM → SSD JSON Freeze)          │   ║
║   │  · Page In  (SSD → RAM Restore)              │   ║
║   └──────────────────────────────────────────────┘   ║
║                                                      ║
║   ┌──────────────────────────────────────────────┐   ║
║   │  IPC Layer                                   │   ║
║   │  · Message Bus  (Agent <-> Agent broadcast)  │   ║
║   │  · Semantic Bus (Vector memory + cosine sim) │   ║
║   │  · Embedding Cache (Hash-based dedup +       │   ║
║   │           Zero-copy pointers)                │   ║
║   │  · Memory GC  (Hourly TTL-based sweep)       │   ║
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
║     │  BERT Embedder│                                ║
║     │ (Safetensors) │                                ║
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

## Request Lifecycle

Every inference request walks through the same pipeline, regardless of which engine is active:

```
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
│    Enforce rate limit               │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 3. CONTEXT FIREWALL                 │  ore-core/src/firewall.rs
│    InjectionBlocker::check()        │
│    PiiRedactor::redact()            │
│    BoundaryEnforcer::encapsulate()  │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 4. SSD PAGER (if stateful)          │  ore-core/src/swap.rs
│    Pager::page_in_history()         │
│    Append new message to context    │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│ 5. GPU SCHEDULER                    │  ore-core/src/scheduler.rs
│    Acquire semaphore permit         │
│    Hot-swap check (skip reload?)    │
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
│ 7. RESPONSE + CLEANUP              │
│    Stream tokens to client          │
│    Pager::page_out_history()        │
│    GpuLease drops → semaphore freed │
└─────────────────────────────────────┘
```

## Crate Dependency Graph

```
ore-common          (shared types: InferenceRequest, InferenceResponse, ModelId)
    ▲
    │
ore-core            (kernel logic: firewall, scheduler, IPC, drivers, native engine)
    ▲
    │
ore-server          (Axum HTTP daemon: routes, auth, state)

ore-cli             (standalone CLI binary, talks to ore-server via HTTP)
```

| Crate | Role | Key Dependencies |
|---|---|---|
| `ore-common` | Wire types shared between crates | `serde`, `uuid` |
| `ore-core` | All kernel logic lives here | `tokio`, `dashmap`, `candle-*`, `regex`, `async-trait` |
| `ore-server` | HTTP interface to the kernel | `axum`, `tokio`, `ore-core` |
| `ore-cli` | Interactive command-line client | `clap`, `dialoguer`, `reqwest`, `hf-hub` |

## Workspace Layout

```
ore-system/
├── ore-common/              Shared types
├── ore-core/                Kernel logic
│   ├── driver.rs            HAL trait (InferenceDriver)
│   ├── firewall.rs          Context firewall (PII, injection, boundary)
│   ├── ipc.rs               MessageBus, SemanticBus, RateLimiter
│   ├── scheduler.rs         GpuScheduler with RAII GpuLease
│   ├── swap.rs              SSD Pager (context persistence)
│   ├── registry.rs          App manifest registry
│   ├── external/            External inference drivers
│   │   └── ollama.rs        OllamaDriver (HTTP proxy)
│   └── native/              Native Candle Engine
│       ├── mod.rs           NativeDriver (GGUF loading + hardware detection)
│       ├── engine.rs        OreEngine enum (Llama/Qwen) + ActiveEngine
│       ├── gguf_tokenizer.rs GGUF metadata tokenizer extractor
│       └── models/          Architecture-specific model loaders
│           ├── llama.rs     Llama family loader
│           ├── qwen.rs      Qwen2 family loader
│           └── bert.rs      BERT embedder (Safetensors)
├── ore-server/              HTTP daemon
│   ├── main.rs              Boot sequence, router, GC scheduler
│   ├── state.rs             KernelState + OreConfig
│   ├── middleware.rs        Bearer token auth middleware
│   ├── payloads.rs          Request payload structs
│   └── handlers/            Route handlers
│       ├── system.rs        Health, ps, ls, agents, manifests, pull, load, expel
│       ├── inference.rs     ask_ai (secured + paged), run_process (streamed)
│       └── ipc.rs           Semantic bus share/search, agent messaging
├── ore-cli/                 CLI tool
│   ├── main.rs              Command dispatch + ore pull/top/run/etc.
│   ├── cli.rs               Clap argument definitions
│   ├── interactive.rs       Interactive wizards (init, manifest)
│   └── utils.rs             HTTP helpers, token reader
├── manifests/               App permission manifests (.toml files)
├── models/                  Downloaded model weights
├── tokenizers/              Global tokenizer JSONs
├── swap/                    SSD page files for agent context
├── ore.toml                 System configuration
├── Cargo.toml               Workspace config + release profile
└── rust-toolchain.toml      Pinned Rust 1.93.0
```

## Design Principles

1. **Security First** - Every prompt is firewalled. Every request is authenticated. Every agent is sandboxed by its manifest. The kernel assumes agents are adversarial.

2. **Zero-Copy Architecture** - The Native Engine achieves sub-50ms instant boot times by utilizing `memmap2` to stream weights directly from the SSD to the GPU, bypassing system RAM bottlenecks. Additionally, the GPU scheduler detects when the requested model is already loaded (hot-swap) and shares the instance instead of reloading. RAII-based `GpuLease` ensures the semaphore is always released, even on panics.

3. **OS-Style Memory Management** - Idle agent context is paged to SSD (`swap/` directory) and restored on demand, just like an OS page file. The `SemanticBus` runs hourly garbage collection to evict stale embeddings and pipe data.

4. **Driver Abstraction** - The `InferenceDriver` trait decouples all kernel logic from the physical inference engine. Swap between Native Candle and Ollama with a single config change. Add new backends by implementing 9 trait methods.

5. **Manifest-Driven Permissions** - Every agent declares its permissions in a TOML manifest. The kernel enforces these at the syscall level - not in the application. No manifest = no access.

---

**Next:** [Getting Started →](./getting-started.md)
