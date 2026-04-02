# ORE Kernel - Development Roadmap

ORE is currently in **v0.1.0 (Alpha)**. We have successfully built the proxy layer, the zero-trust registry, the contextual firewall, and the native Rust execution engine (`candle`). 

However, current agent frameworks (OpenClaw, AutoGPT) treat local laptops like infinite cloud endpoints, resulting in VRAM thrashing, OOM crashes, and massive data duplication. **ORE is the OS layer designed to arbitrate this hardware.**

This roadmap outlines our trajectory toward true AI Virtualization. **Pull Requests, architectural critiques, and community contributions are highly welcome.**

---

## Phase 1: The Foundation (Completed)
- [x] **Universal Driver Abstraction:** Pluggable HAL supporting `Ollama` (HTTP) and `Candle` (Native Rust).
- [x] **Context Firewall:** Ingress prompt inspection, PII redaction, and structural boundary enclosure to prevent prompt injections at the API layer.
- [x] **Zero-Trust App Registry:** Strict `.toml` manifest enforcement for agent permissions (File I/O, Network, IPC targeting).
- [x] **The Semantic Bus (Tier 2 IPC):** Zero-idle-RAM vector memory sharing using `all-MiniLM`, `DashMap`, sliding windows, and masked mean pooling.
- [x] **The Message Bus (Tier 1 IPC):** High-speed `mpsc` text routing for agent-to-agent coordination.
- [x] **Native Package Manager:** The `ore-cli` tool for streaming weights and extracting metadata directly from Hugging Face.
- [x] **Instant Boot via `mmap`:**
  - *Goal:* Boot a 4GB model in 50 milliseconds.
  - *Implementation:* Ensure `memmap2` is aggressively utilized so the OS streams only the required weights directly from SSD to GPU, bypassing system RAM bottlenecks.

---

## Phase 2: The VRAM Manager (Current Focus)
This phase transforms ORE from an API proxy into a true bare-metal Memory Manager, eliminating the "VRAM Context Wall."

- [ ] **True KV-Cache Paging (Virtual Memory for AI):**
  - *Goal:* Current JSON history swapping is safe but slow. We need instant suspend/resume.
  - *Implementation:* Intercept the physical KV-Cache tensors mid-generation inside the `NativeDriver`. Serialize them using `candle_core::safetensors` (or `bincode`/`rkyv`), write them directly to NVMe SSDs, and clear the VRAM. Agents can now "sleep" on the SSD with zero RAM footprint and wake up in milliseconds.
- [ ] **VRAM Bin Packing (Multi-Tenancy):**
  - *Goal:* Stop strictly evicting models if there is physical space available.
  - *Implementation:* Upgrade the `GpuScheduler` to read total available VRAM. If Agent A requests Qwen (0.4GB) and Agent B requests Llama (1.2GB) on an 8GB GPU, load them *alongside* each other for 0-second context switching.
- [ ] **LoRA Multiplexing (Copy-On-Write for Intelligence):**
  - *Goal:* Run 10 different agent personalities using the VRAM of just 1 model.
  - *Implementation:* Hold the Base Model in VRAM once. Hot-swap tiny 50MB LoRA adapters into the computation graph per-request based on the Agent's `.toml` manifest.
- [ ] **Expand Native Architecture Support:**
  - *Goal:* Expand the `OreEngine` polymorphic router.
  - *Implementation:* Add native `candle` support for Mistral, Gemma, Deepseek, Microsoft Phi, and GLM.

---

## Phase 3: The Managed Swarm & Execution
Giving agents hands, and teaching them how to coordinate autonomously.

- [ ] **WASM Tool Sandbox (Zero-Trust Execution):** 
  - *Goal:* Agents need to execute code (Python/Bash) safely without destroying the host machine.
  - *Implementation:* Integrate `wasmtime` to spin up isolated WebAssembly rings. If an agent's manifest forbids network access, the sandbox physically drops the packets.
- [ ] **Event-Driven Kernel Routing:**
  - *Goal:* Eliminate the need for messy `swarm_manager.py` loops.
  - *Implementation:* Add `wake_up_on_pipe_update = "pipe_name"` to manifests. When Agent A drops data into a Semantic Pipe, the ORE Kernel automatically fires a wake-lock signal to Agent B.
- [ ] **Frictionless Adoption (ORE SDKs):**
  - *Goal:* Developers using LangChain/OpenClaw won't write raw HTTP requests.
  - *Implementation:* Release `pip install ore-sdk` and `npm install @ore/sdk` to provide native Python/TS wrappers for the Semantic Bus, GPU Locks, and Message Bus.

---

## Phase 4: The Infinite Computer (Future Vision)
Making hardware limits obsolete.

- [ ] **The Cloud Filesystem (`hf-mount` Driver):**
  - *Goal:* Instant inference on any model in the world without downloading 10GB files.
  - *Implementation:* Integrate a `HuggingFaceMountDriver` using FUSE/NFS. ORE mounts the Hugging Face hub as a local `/models` directory and lazily streams byte-ranges directly into the GPU on-demand.
- [ ] **P2P Local Mesh (AirDrop for Compute):**
  - *Goal:* Combine the idle Mac Mini in the living room with the Windows PC in the bedroom into a single logical GPU cluster.
- [ ] **Graceful Hardware Degradation:**
  - *Goal:* If the local GPU overheats or VRAM is exhausted, ORE dynamically strips PII via the Firewall and securely routes the prompt to an external cloud API as a seamless fallback.

---

## 🤝 Contributing
If you are a Systems Engineer, Rustacean, or ML Infrastructure enthusiast, pick any unchecked item above! 

*Specifically looking for critiques and PRs on heavily optimizing `mmap` tensor deserialization for the SSD Pager, and mapping complex architectures (SwiGLU/RoPE) in the native `candle` engine.*

Please open an Issue first to discuss the architecture before submitting a massive PR.