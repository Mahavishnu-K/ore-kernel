# Why Ore?

This page explains the problems Ore is built to solve and the design choices behind each subsystem. For how the pieces fit together, see [Architecture](./architecture.md). For the code-level details, see [Kernel Internals](./kernel-internals/).

---

## The problem

Local AI stacks were built for one user talking to one model. Multi-agent workloads break that assumption in three ways:

**No permission boundary.** Agent frameworks typically run tools with the full privileges of the host process. A prompt injection that reaches a tool-using agent can read `.env` files, SSH keys, or anything else the user can read.

**No arbitration of the GPU.** Inference engines don't coordinate between clients. Two agents loading models and building KV caches at the same time can exhaust VRAM, and the usual result is an out-of-memory error rather than a queue.

**No sharing.** Each application tends to ship or download its own copy of the same weights, run its own embedding model, and keep its own vector store. Three apps can mean three copies of a 7B model.

Ore's answer is to put a single daemon between applications and the hardware. Applications never talk to the GPU directly. They talk to Ore, and Ore enforces the rules.

| Concern | Typical local setup | With Ore |
|---|---|---|
| Permissions | Agents inherit the host process's access | Per-agent TOML manifests enforced by the kernel |
| GPU contention | Concurrent loads can OOM | Semaphore leases, VRAM admission control, LRU eviction |
| Model sharing | Each app loads its own copy | One loaded instance shared across apps |
| Context persistence | History lost on restart; prefill recomputed on resume | Chat history and KV cache paged to disk and restored |
| Tool execution | Docker, or unsandboxed host code | WebAssembly sandbox with fuel limits, VFS, and an egress proxy |
| Shared memory between agents | External vector database | Built-in Semantic Bus with manifest-gated pipes |
| Rate limiting | None | Per-agent `max_tokens_per_minute` |
| PII / injection filtering | None | Heuristic regex filters (best-effort, see [Security Model](./security-model.md)) |

---

## 1. WebAssembly tools instead of containers

Giving an agent the ability to run code usually means a Docker container or a cloud VM. Ore runs tools as WebAssembly modules inside the kernel using `wasmtime`.

- **Language-agnostic tools.** `ore mktool` compiles Rust, Go, Python, JavaScript, TypeScript, Zig, C, and C++ to `.wasm`. An agent written in any language can call a tool written in any other.
- **CPU fuel limits.** Each execution gets a fuel budget (`max_cpu_instructions` in the manifest, default 5 billion). A runaway loop traps instead of hanging the host.
- **No raw sockets.** The sandbox has no TCP/UDP access. Outbound HTTP goes through a Layer 7 proxy in the kernel that checks domain, method, and path against the manifest's `[[network.rules]]`. Responses are written to the sandbox's temp directory.
- **Capability-based file system.** Only directories listed in the manifest are mounted. Read-only paths are mounted without write permission at the WASI boundary.
- **AOT caching.** Modules are compiled to native code on first run and cached as `.cwasm`. Later runs memory-map the cached artifact, so start-up is around 10 ms and the OS page cache shares the code between concurrent instances.
- **Portable artifacts.** A `.wasm` tool built on one OS runs on any other without a Python environment, dependency tree, or CUDA install.

**What it is not for:** WebAssembly is a compute environment. Tools should take data in through stdin or files, compute, use `ore.fetch` for network access, and write data out. Browsers, GPU training loops, and native database drivers don't belong here.

Details: [WASM Sandbox](./kernel-internals/wasm-sandbox.md).

---

## 2. Memory Fusion with `ore-ld`

Stdin/stdout piping works for most tools, but it forces serialization. If a host module holds a large buffer and a tool needs to transform it, copying it out as JSON and back is the dominant cost.

The usual alternatives each give something up:

- **Native FFI** shares memory and is fast, but a crash in the library takes down the host, and the library must be rebuilt for each OS.
- **Microservices** are isolated, but every call pays for serialization and a network hop.

`ore-ld` is a dynamic linker for WebAssembly shared objects (`.wasi.so`). A host module calls `ore_dlopen` / `ore_dlsym` at runtime. The linker grows the host's function table and memory, places the plugin in the host's linear memory, and resolves its symbols. The host can then pass a pointer instead of a copy.

- **No copying.** The plugin operates on the host's memory in place.
- **Fault containment.** A memory violation in the plugin is a WebAssembly trap. It stops the execution without crashing the kernel or the host OS.
- **Portable.** A plugin compiled on macOS runs on Linux unchanged.
- **Hot-swappable.** A long-running host can load a new version of a plugin without losing its own in-memory state.

Plugins are limited to Rust, C, C++, and Zig. These toolchains can emit position-independent WebAssembly that uses an imported memory and table. Garbage-collected runtimes (Python, JS, Go) expect to own the whole linear memory, so they can't share one with another module's allocator. They use regular `.wasm` tools instead. (A bridge that lets them call plugins through the kernel is planned but not shipped yet.)

The SDKs handle the ABI boilerplate: `ore.h` with the `Ore_BIND` macro for C/C++, `ore-sys` with `ore_export!` / `ore_bind!` for Rust, and `ore.zig` for Zig. Build a plugin with `ore mktool math.c --shared` and a host with `ore mktool agent.rs --host`.

Details: [Polyglot Memory Fusion](./kernel-internals/polyglot-memory-fusion.md) · [Building Plugins](./building-plugins.md).

---

## 3. GPU scheduling

On a GPU with limited VRAM, the constraint for multiple agents is usually space rather than speed. Model weights plus several KV caches can exceed what fits, and the engine fails with an out-of-memory error.

Ore's scheduler:

- Hands out a `GpuLease` (an RAII semaphore permit) before inference. When the lease drops, its KV-cache reservation is released.
- Reads real VRAM usage through NVML on NVIDIA GPUs (falling back to system memory elsewhere) and admits a request only if its estimated KV cache fits.
- Estimates KV-cache size from the model's GGUF geometry, with overrides in `ore.toml`.
- Loads multiple models side by side when they fit, and evicts the least recently used idle model when they don't.

Combined with KV-cache paging (below), this means agents queue for the GPU instead of crashing it.

Details: [GPU Scheduler](./kernel-internals/gpu-scheduler.md).

---

## 4. KV-cache paging

When an inference engine switches from agent A to agent B, it normally discards A's KV cache. When A resumes, the engine re-runs prefill over A's whole history. On CPU this is the dominant cost.

Ore serializes an agent's KV cache to `.safetensors` on disk when it's paged out and restores it on the next request, so prefill isn't repeated.

**Measured:** on a Ryzen 7 with 16 GB RAM (CPU only), resuming an agent took **37.03 s** when recomputing prefill and **2.41 s** when restoring the saved KV cache. That's **15.4× faster**.

Because the cache lives on disk, it also survives restarts. An agent can resume a long conversation after the machine has been rebooted without re-processing its history.

Agents opt in with `stateful_paging = true` in their manifest. Background compaction caps disk usage and falls back to summarized JSON history when limits are reached.

Details: [Memory Management](./kernel-internals/memory-management.md).

---

## 5. Semantic Bus

Sharing knowledge between agents usually means running a separate vector database. Ore has one built in.

- An agent writes text to a named pipe. Ore loads an embedding model (BERT / all-MiniLM or Nomic v1.5, from safetensors), embeds the chunks, and unloads the model, so it uses no RAM while idle.
- Other agents search the pipe by similarity, with optional time-decay scoring and per-source filtering.
- Embeddings are cached by content hash and shared through `Arc` pointers, so the same text is embedded only once.
- Read and write access to each pipe is gated by `allowed_semantic_pipes` in the manifest.
- Pipes can be persisted to disk, and a TTL-based sweep removes stale data.

Details: [IPC & Semantic Bus](./kernel-internals/ipc-and-semantic-bus.md).

---

## Capacity estimates

> [!NOTE]
> These are **back-of-envelope estimates**, not benchmarks. They assume every agent uses the same model, KV caches are paged to disk between turns, and agents take turns on the GPU. Real numbers depend on context length, quantization, and storage speed. Measured results are welcome as PRs.

With paging, the limit on agent count moves from VRAM to time. Each active agent needs a turn on the GPU, so the practical ceiling is how many turns per minute your workload can tolerate.

| | Mid-range laptop | High-end workstation |
|---|---|---|
| **Hardware** | Ryzen 5 / i5, 16 GB RAM, 4–6 GB RTX 3050, NVMe | RTX 4090 (24 GB) + 64 GB RAM, or M3 Max 128 GB |
| **Model** | 3–4B at Q4_K_M (~2.5 GB VRAM) | 30–32B |
| **Assumed generation speed** | ~40 tok/s | ~80–120 tok/s |
| **Assumed KV swap time** | ~0.15 s | ~0.05 s |
| **Time per agent turn** | ~1 s | ~0.4 s |
| **Dormant agents** | Limited by disk space | Limited by disk space |
| **Active agents (~1 turn/min each)** | ~40–50 | ~150 |

This suits background workloads such as research, code review, or QA swarms, where each agent can wait its turn. It doesn't suit workloads where every agent needs low latency at once.
