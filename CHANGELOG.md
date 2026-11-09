# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **WebAssembly Host Syscalls (`ore::fetch` & `ore::fetch_async`)**:
  - `ore::fetch` now supports passing custom HTTP headers via guest memory pointer and length (`headers_ptr`, `headers_len`) as a JSON string, parsed safely into `HashMap<String, String>` and forwarded to reqwest.
  - Implemented asynchronous non-blocking fetch host syscall (`ore::fetch_async` in host linker, exported to guests as `ore_fetch_async` / `_ore.fetch_async` / `__ore__.fetchAsync`). Dispatches the egress request in a background thread and immediately returns status 0 to unblock the guest, writing streaming response metadata to `<filename>.meta` and completion status to `<filename>.done` (or 502 sentinel on failure).
  - Enhanced memory boundary reading in `ore-core/src/sandbox/shims.rs`: `read_mem_bytes`, `read_mem_string`, and robust `parse_headers_json` validation preventing out-of-bounds reads.
  - Integration tests in `tests/wasm_sandbox_tools/network_interception.rs` verifying both sync and async fetch host functions, custom headers, and response streaming.
- **WASM Guest Runtimes Infrastructure (`guests/`)**:
  - Added source code, native C extensions, Dockerfiles, and reproducible build pipelines for kernel guest runtimes:
    - `guests/python/`: CPython 3.12.3 compiled for `wasm32-unknown-wasi` via wasi-sdk 22 in Docker (`Dockerfile.build`), cross-compiled with zlib 1.3.1 for WASI, built-in native C extension `_ore_module.c` exposing `_ore.fetch()` and `_ore.fetch_async()`, packed standard library using `wasi-vfs` with unchecked-hash `.pyc` (PEP 552) to avoid stdlib re-compilation and preserve CPU fuel budget (~700M instructions vs 5B fuel limit), and stripped DWARF debug symbols down to ~27 MB.
    - `guests/quickjs/`: QuickJS-NG v0.18.0 compiled for `wasm32-wasi` via Zig 0.17 (`build.sh`), integrating native C bridge `quickjs-ore.c` / `quickjs-ore.h` injecting `globalThis.__ore__` (`__ore__.fetch`, `__ore__.fetchAsync`), and 2 MiB execution stack down to ~5 MB.
    - Comprehensive guide in `guests/README.md` detailing runtime architecture, C bridge design, build instructions, and verification scripts (`run_inception.py`, `run_network.py`).
    - Added manifests and scripts, e.g., `manifests/web_scrapper.toml` for sandboxed web scraping agents utilizing Inception Mode.
- **TypeScript JIT Transpilation & Smart NPM Caching**:
  - Upgraded `ore-server/src/execution/script.rs`:
    - Pure TypeScript execution without external dependencies now uses a fast, dedicated staging directory (`_transpile_ts`) directly transpile-bundled with esbuild, eliminating unnecessary overhead.
    - Deterministic SHA-256 package requirement hashing for NPM dependencies: dependency lists are sorted, hashed, and cached in `cache/npm/<hash>/`. Cache hits completely bypass `npm install`.
    - Enhanced error handling when launching `npm` or `esbuild` with descriptive `KERNEL ERROR` reporting to STDERR.
- **Cognitive MMU suite** (`examples/ipc_mmu/`): 20 end-to-end scenarios run by a six-agent swarm with its own manifests; a labelled offline corpus (115 documents, 132 queries: near misses, multi-document, technical, long, CJK, unanswerable, adversarial) with Recall@k, MRR, nDCG@10, and CRAG threshold calibration; `web_search.py` (Wikipedia + DuckDuckGo through `/net/fetch`, with agent-side CRAG); HTTP benchmarks with p50/p95/p99; a `criterion` bench for the in-process search core (`cargo bench -p ore-core --bench cognitive_mmu`); and an offline firewall test over the adversarial cases (`cargo test -p ore-core --test firewall_corpus`).
- **Published benchmarks** (`docs/benchmarks.md`): Cognitive MMU results on a Ryzen 7 5800HS (CPU only) from the `ipc_mmu` suite and the criterion bench: search latency by pipe size, concurrency, branches, Mode A vs C, Message Bus, retrieval quality, CRAG calibration and chunking. Headline numbers are in the README.
- **Cognitive MMU**: the Semantic Bus is now a kernel-native agent memory system (`ore-core/src/ipc/`):
  - **Hybrid retrieval**: a BM25 sparse index alongside dense search, fused with Reciprocal Rank Fusion; scores are normalized to [0, 1].
  - **Memory tiers**: every chunk carries a `MemoryCategory` (`fact`, `skill`, `directive`, `general`), set on `/ipc/share` and filterable on `/ipc/search`.
  - **Sentence-window chunking** (`chunk_strategy: "sentence_window"`): each sentence is embedded on its own, while a wider `context_payload` is stored and returned.
  - **Copy-on-write branches**: `POST /ipc/pipe/branch`, `/ipc/pipe/diff`, and `/ipc/pipe/merge`.
  - **Storage modes**: Mode A (full-precision vectors in RAM), Mode C (`int8` vectors in RAM + `f32` vectors in a memory-mapped `.f32vecs` file), with automatic A → C migration at `max_hot_chunks`; Mode B (product quantization + on-the-fly re-embedding) is experimental.
  - **RAPTOR summary trees**: `POST /ipc/pipe/raptor_build` clusters a pipe, summarizes each cluster with the agent's model, and recurses up to `max_levels`.
- **Autonomous RAG**: with `[rag].auto_rag`, `/infer` and `/run` retrieve context before inference, with optional HyDE (hypothetical-answer search) and a CRAG fallback that runs a sandboxed tool when confidence is below `confidence_threshold`. `/ipc/search` accepts per-request overrides.
- **`[rag]` manifest section**: `auto_rag`, `enable_hybrid`, `enable_hyde`, `confidence_threshold`, `crag_fallback_tool`, `vector_mode`, `max_hot_chunks`.
- **`POST /infer`**: the stateful agent route takes a JSON body and identifies the agent with the `X-App-ID` (or `X-Agent-ID`) header. `GET /infer/:prompt` remains as a legacy alias for quick `curl` checks.
- **`ore-ld` plugin lifecycle**: `ore_dlclose` (nulls the plugin's table slots so stale pointers trap, and recycles its memory slab) and `ore_dlerror`, with SDK wrappers: `ore_close` / `ore_error` (C), `Plugin::close` / `Plugin::last_error` and automatic close on `Drop` (Rust), `close` / `lastError` (Zig).
- Plugin lifecycle test tools in `tests/memory_fusion/` and `tests/cross_lang_memory_fusion/`, run by `examples/wasm_agents/run_plugin_lifecycle.py`.
- **Managed Zig toolchain**: `ore mktool` installs Zig 0.17.0 automatically when no `zig` is available, downloading from community mirrors and verifying the minisign signature and signed file name. `ORE_ZIG` can point at a specific binary.
- `InferenceDriver::count_tokens` (exact counts from the native tokenizer) and `device_name`; context limits use a fast estimate and switch to exact counts near the limit.
- `execution.is_coding_agent` manifest field (and manifest-wizard option), which lets SQL and code text pass the injection firewall.
- `"*"` wildcard in `allowed_models`; the manifest wizard offers `* (any installed model)`.
- `docs/why-ore.md` and a "Prior Art & Research Inspirations" section in the README.
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
- **Nomenclature Standardization**: Standardized capitalization of "Ore" across documentation, crate descriptions (`Cargo.toml` in `ore-cli`, `ore-common`, `ore-sys`), code comments, and example files.
- **Repository Cleanups**: Removed deprecated binary screenshots and diagrams from `docs/img/`. Added ignore rules to root `.gitignore` and `guests/.gitignore` for precompiled `.cwasm` artifacts, `quickjs-src/`, and WebAssembly binaries.
- **Batched embeddings**: the BERT and NomicBert embedders run texts in length-bucketed, padded batches (up to 32 texts, bounded by padded tokens and attention size) instead of one forward pass per text. Padding is masked out of attention and pooling, so vectors match single-text embedding; `ore-core/tests/embedder_batching.rs` checks this against the installed models.
- **Nomic task prefixes**: `system-embedder` (NomicBert) now embeds stored text with `search_document: ` and search queries with `search_query: `, as the model was trained. A HyDE excerpt counts as a document. `InferenceDriver::generate_embeddings` takes an `EmbedTask`, and the embedding cache keys query vectors apart from document vectors. MiniLM and the Ollama engine are unchanged. **Re-ingest persistent pipes written with `system-embedder` before this change**: their vectors have no prefix, so queries match them less well.
- **Nomic input capped at 2,048 tokens** (the model's trained length, down from 8,192): one very long text needs about 200 MB of attention scores instead of about 3 GB.
- **Embedder keep-alive**: the embedder stays in RAM between requests and unloads after `[system] embedder_keep_alive_secs` (default 60) without use, instead of loading from disk on every share and search. `0` restores the old unload-after-every-request behaviour. While loaded, the embedder is listed by `/ps`, `/ls` and `/health`.
- **`ipc.rs` split into the `ipc/` module**: `semantic.rs`, `bm25.rs`, `chunking.rs`, `quant.rs`, `message_bus.rs`, `rate_limiter.rs`.
- `search_pipe` returns a `SearchOutcome` (`results`, `top_dense_score`, `top_fused_score`); equal scores are ordered by chunk id, so results are deterministic. `/ipc/search` results include `context_payload`, `category`, and `is_crag_fallback`.
- **Breaking:** the old `/ask/:prompt` route is replaced by `POST /infer` and `GET /infer/:prompt`. Errors on these routes now use HTTP status codes (`400`, `401`, `403`, `429`, `503`). `ore_client.ask()` uses `POST /infer`.
- `/infer` and `/run` share one Auto-RAG pipeline; HyDE runs under its own identity (`<app_id>::hyde`) with a GPU lease.
- **Sandbox result reporting**: outcomes are classified by wasmtime's typed error instead of message text. A non-zero exit, a fuel trap, or any other trap ends the output with a single `KERNEL ERROR:` line; output printed before a fuel trap is kept.
- `ore mktool` compiles C++ with `-fno-exceptions` (WASI's libc++ has no exception support).
- `[models."<name>"].kv_cache_mb_per_1k` is now applied per 1,000 tokens of context, as its name says.
- A manifest that fails to parse is skipped with a `[SECURITY ALERT]` instead of stopping the kernel from booting; duplicate `app_id`s are reported.
- The Message Bus is bounded at 1,024 pending messages per agent and rejects new messages when full.
- README and `docs/` rewritten to match the code.
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
- **`/ipc/share` hung forever** (and so did CRAG ingestion): the handler held `embedder_lock` and then called `generate_embeddings_scoped`, which takes the same non-re-entrant lock.
- The BERT embedder truncated input at 8192 tokens instead of the model's 512 positions, so a long text failed to embed.
- **Embedding unloaded the chat model**: the native driver's `unload_model` ignored the model name, so every share and search evicted the loaded LLM and its KV cache. It now unloads only the model it is given.
- **KV cache reused on the wrong tokens**: the native engine resumed a cached conversation at the cache's length without checking that the cached tokens matched the new prompt. For an agent with `auto_rag` and `stateful_paging`, the cache held the previous turn's retrieved context but the stored history didn't, so the next turn resumed mid-sequence and answered from the old context, with no error. The engine now records the token ids its cache holds, in RAM and in the SSD page (`ore_token_ids`), and reuses only the longest prefix shared with the next prompt (as in llama.cpp and vLLM prefix caching). This also covers compacted histories and answers that re-tokenize differently. On a real model, reused and SSD-restored caches now give the same next-token logits as a cold prefill (`ore-core/tests/kv_prefix_reuse.rs`). KV pages written by older versions have no token record and are rebuilt once.
- **Unloading a model discarded its KV cache**: a scheduler eviction or `ore expel` dropped a paging agent's cache without saving it. It is now written to SSD first, like an agent switch or an idle timeout.
- A failed tokenize or forward pass during generation panicked while holding the engine lock, which poisoned it for every later request. It now returns an error.
- **Semantic Bus persistence**:
  - the first write after a restart no longer replaces a persistent pipe on disk (pipes are loaded before writes);
  - pipe files are written with `fsync` + atomic rename + directory sync;
  - unreadable pipe files are quarantined as `.corrupt-<timestamp>` instead of being overwritten;
  - concurrent first access after a restart no longer drops writes.
- **Mode C**: pipes are persisted (`.sq8pipe`), keep their vector dimension and limits across restarts, and are no longer expired by TTL when persistent. `MmapVectorStore` rejects vectors of the wrong dimension instead of reading out of bounds.
- **Copy-on-write merge**: no duplicated chunks after a merge, no lost deltas when a merge fails (validation runs first, with rollback), and no disk I/O under the pipe lock.
- **BM25**: evicted chunks are removed from the index, so it no longer grows without bound or skews scores.
- **GC** no longer deletes a freshly created, still-empty branch.
- **RAPTOR**: builds every level (not just one), embeds summaries with their cluster centroid, uses the agent's concrete model, and takes a GPU lease.
- **Possible deadlock** when searching a branch whose base had to be paged in.
- **Scheduler**: model files are found for names containing `:` (`models/qwen2.5-0.5b/`), so KV-cache sizing uses real GGUF geometry instead of the fallback.
- An `allowed_models` list starting with `"*"` no longer makes `/infer`, `/compact`, or RAPTOR request a model named `*`.
- **CRAG**: the query is passed as `argv[1]` (not after a duplicated tool name), and failed tool output (`KERNEL ERROR`) is never stored as knowledge.
- **Zig tools** that trapped were reported as clean exits.
- **`ore-ld`**: `ore.h` declared `ore_close` / `ore_error` without their imports (breaking every C/C++ host); `ore_dlerror` now reports `dlopen` / `dlsym` failures; recycled memory slabs are zeroed; a failed `dlopen` no longer leaks its slab.
- `/run` FIFO pruning keeps the first message (the compaction summary), as `/infer` does.
- Clean up of temporary `.tmp_build` directories natively post-execution.

### Security
- **Egress proxy**: every redirect hop is re-checked against the agent's rules; `allowed_paths` is enforced; loopback, link-local, and cloud-metadata addresses (e.g. `169.254.169.254`, `[::ffff:127.0.0.1]`, `http://2130706433/`) are blocked unless `allow_localhost_access = true`; only `http` / `https` are allowed.
- `/run` enforces the agent's `allowed_models` (`"*"` allows all; the `terminal_user` manifest used by `ore run` ships with `"*"`).
- Context retrieved by Auto-RAG and CRAG passes the context firewall under the reading agent's manifest before it reaches a prompt.
- `/infer` is rate-limited with the same per-agent quota as `/run`.
- Pipe merges require access to both the branch and the base pipe.

## [0.1.0] - 2026-07-22

### Added
- Initial project structure containing `ore-core`, `ore-server`, and `ore-cli`.
- Documentation in the `docs/` directory.

[Unreleased]: https://github.com/Mahavishnu-K/ore-kernel/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Mahavishnu-K/ore-kernel/releases/tag/v0.1.0
