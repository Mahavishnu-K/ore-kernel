# ORE Roadmap

ORE is **alpha** (v0.1.0). This page tracks what's built, what's next, and where help is most useful. Items are checked only when they're implemented in the code on `main`/`dev`. See the [CHANGELOG](CHANGELOG.md) for release-by-release detail.

**Legend:** ✅ done · 🚧 in progress or partial · ⬜ not started

| Phase | Status |
|---|---|
| [1. Foundation](#phase-1-foundation) | ✅ Done |
| [2. VRAM manager](#phase-2-vram-manager) | 🚧 Mostly done |
| [3. Sandboxed execution](#phase-3-sandboxed-execution) | ✅ Done |
| [4. Hardening](#phase-4-hardening-current-focus) | 🚧 **Current focus** |
| [5. Agent coordination and SDKs](#phase-5-agent-coordination-and-sdks) | ⬜ Planned |
| [6. Beyond one machine](#phase-6-beyond-one-machine) | ⬜ Exploratory |

---

## Phase 1: Foundation

✅ The kernel daemon, permission model, and inference backends.

- [x] **Driver abstraction (HAL):** the `InferenceDriver` trait, with Ollama (HTTP) and native Candle backends.
- [x] **Native inference:** pure-Rust GGUF inference for Llama, Qwen2, Qwen3, and Qwen3-MoE on CPU, CUDA, and Metal, with fused MoE kernels.
- [x] **App registry and manifests:** per-agent TOML manifests covering models, file system, network, execution, and IPC, validated at boot.
- [x] **Auth and rate limiting:** bearer-token middleware on every route, plus per-agent `max_tokens_per_minute`.
- [x] **Context firewall:** regex-based PII redaction and prompt-injection heuristics.
- [x] **Message Bus (IPC tier 1):** agent-to-agent messaging gated by `allowed_agent_targets`.
- [x] **Semantic Bus (IPC tier 2):** shared vector memory with on-demand embedders (BERT / all-MiniLM, Nomic v1.5), an embedding cache, chunking strategies, time decay, persistence, and TTL garbage collection.
- [x] **CLI and package manager:** `ore init`, `ore pull` (streams GGUF and safetensors from Hugging Face), `ore run`, `ore manifest`, and `ore top`.

## Phase 2: VRAM manager

🚧 Turning ORE from an API proxy into a memory manager for the GPU.

- [x] **KV-cache paging:** physical KV caches are serialized to `.safetensors` on disk and restored on the agent's next turn, with background compaction and JSON-history fallback.
- [x] **VRAM bin-packing:** `ModelRegistry` + `MemoryAccountant` + NVML readings let multiple models share VRAM, with admission control from GGUF geometry and LRU eviction.
- [ ] **Priority scheduling:** `gpu_priority` is parsed from manifests but not yet used by the scheduler. Agents should be admitted by priority, not just arrival order.
- [ ] **Non-NVIDIA VRAM readings:** on AMD and Intel discrete GPUs the scheduler currently falls back to *system* RAM figures. This needs real VRAM providers (e.g. ROCm SMI, Level Zero).
- [ ] **LoRA multiplexing:** hold one base model in VRAM and hot-swap small LoRA adapters per agent, as declared in the manifest.
- [ ] **More native architectures:** Mistral, Gemma, Phi, DeepSeek, and GLM loaders for the Candle engine.

## Phase 3: Sandboxed execution

✅ Giving agents tools without giving them the host.

- [x] **WASM sandbox:** `wasmtime` execution with CPU fuel limits, run on a dedicated blocking thread.
- [x] **Capability-based file system:** only manifest-listed directories are mounted; read-only paths have no write permission at the WASI boundary.
- [x] **Layer 7 egress proxy:** no raw sockets; outbound HTTP is checked against domain, method, and path rules.
- [x] **AOT caching:** compiled modules are cached as `.cwasm` and memory-mapped on later runs (~10 ms start-up), and invalidated by file timestamps.
- [x] **Polyglot toolchain:** `ore mktool` builds tools from Rust, Go, Python, JS, TS, Zig, C, and C++, including whole project directories.
- [x] **Script mode (Inception):** agents can run generated Python/JS scripts in the sandbox, with runtime shims for `requests`, `httpx`, and CommonJS.
- [x] **`ore-ld` dynamic linker:** `ore_dlopen` / `ore_dlsym` load `.wasi.so` plugins into a host's linear memory for zero-copy calls, with SDKs for Rust (`ore-sys`), C/C++ (`ore.h`), and Zig (`ore.zig`).
- [x] **Host shell mode:** an explicit escape hatch (`can_execute_shell = true`) that flags the agent as UNSAFE.

## Phase 4: Hardening (current focus)

🚧 Making the existing features trustworthy before adding more. This is where contributions help most.

- [ ] **Adversarial sandbox tests:** path traversal and symlink escapes from mounted directories, fuel exhaustion, egress-rule bypasses (redirects, IP literals, IDN hostnames, `localhost` aliases), and malformed `.wasm` / `.wasi.so` input.
- [ ] **Firewall tests and tuning:** measure false positives (for example, SQL questions tripping the SQL-injection rule, or long numbers matched as card numbers) and false negatives on paraphrased injections. Document clearly what the firewall can and can't catch.
- [ ] **Panic safety:** replace `unwrap()` on locks and I/O in kernel paths with proper errors, so one bad request can't poison a mutex or take down the daemon.
- [ ] **Reproducible benchmarks:** a `bench/` harness with fixed models, context lengths, and hardware notes for KV paging, sandbox start-up, and scheduler throughput.
- [ ] **Stable formats:** version the KV-cache and Semantic Bus on-disk formats so upgrades don't silently invalidate saved state.

## Phase 5: Agent coordination and SDKs

⬜ Making ORE easy to adopt and letting the kernel drive agents.

- [ ] **Python and TypeScript SDKs:** publish `ore-sdk` (PyPI) and `@ore/sdk` (npm) wrapping inference, the Message Bus, the Semantic Bus, and sandbox execution. `examples/ore_client.py` is the starting point.
- [ ] **Event-driven wake-ups:** a manifest field such as `wake_up_on_pipe_update = "pipe_name"`, so the kernel notifies an agent when a pipe it watches changes, instead of agents polling.
- [ ] **Streaming IPC:** replace `/ipc/listen` polling with server-sent events or WebSockets.
- [ ] **SDK bridge for `ore-ld`:** let Python and JS agents call `.wasi.so` plugins through the kernel (`ore_bridge_spawn` / `ore_bridge_call` are stubbed out in `linker/mod.rs`).
- [ ] **Framework adapters:** integrations for common agent frameworks such as LangChain, AutoGen, and CrewAI.

## Phase 6: Beyond one machine

⬜ Exploratory ideas. Design discussion is welcome before any code.

- [ ] **Lazy model mounting:** stream model byte ranges from the Hugging Face Hub on demand instead of downloading whole files first.
- [ ] **Local compute mesh:** pool GPUs across machines on a LAN into one scheduler.
- [ ] **Cloud fallback:** when local capacity is exhausted, route a request to a remote API, with manifest opt-in and PII redaction applied first.
- [ ] **Multi-GPU scheduling** on a single host.

---

## Contributing

Pick any unchecked item and open an issue to discuss the design before sending a large PR. The **Phase 4** items are the most valuable right now, and several of them (tests, benchmarks) are good first contributions.

See [CONTRIBUTING.md](CONTRIBUTING.md), or join us on [Discord](https://discord.com/channels/1477053099494342755/1477053558879686737) in `#dev-core`.
