# IPC & Semantic Bus (Cognitive MMU)

> Agent-to-agent messaging, and the shared memory system agents read and write: hybrid search, memory tiers, copy-on-write branches, three storage modes, and crash-safe persistence.

**Source:** [`ore-core/src/ipc/`](../../ore-core/src/ipc/) · Handlers: [`ore-server/src/handlers/ipc.rs`](../../ore-server/src/handlers/ipc.rs) · Persistence: [`ore-core/src/memory.rs`](../../ore-core/src/memory.rs)

---

## Module Layout

| File | Contents |
|---|---|
| `mod.rs` | Re-exports and the subsystem's unit tests |
| `message_bus.rs` | `MessageBus`, `AgentMessage` (tier 1: direct messages) |
| `semantic.rs` | `SemanticBus`, `PipeState`, hybrid search, CoW branches, RAPTOR, GC (tier 2: shared memory) |
| `bm25.rs` | `Bm25Index` sparse keyword index and tokenizer |
| `chunking.rs` | `MemoryChunk`, `ChunkPayload`, all chunking strategies |
| `quant.rs` | `Sq8Vector`, `MmapVectorStore` (Mode C), `PqCodebook` (Mode B) |
| `rate_limiter.rs` | Per-agent `RateLimiter` |

All shared maps are `DashMap`s for sharded, low-contention concurrent access.

---

## Message Bus (Tier 1: Direct Messaging)

```rust
pub const DEFAULT_MESSAGE_BACKLOG: usize = 1024;

pub struct AgentMessage { pub from_app: String, pub to_app: String, pub payload: String, pub timestamp: u64 }
```

- Each agent's inbox is a **bounded** `mpsc` channel (1,024 messages by default; `MessageBus::with_capacity` changes it, minimum 1), created on first use.
- `send_message` uses `try_send`, so it never blocks. A full inbox rejects the message with `Target agent '…' queue is full (capacity: 1024). Backpressure applied.`
- `read_message` polls without blocking. `GET /ipc/listen/:app_id` returns the next message or `null`.
- The sender's manifest must list the receiver in `allowed_agent_targets` (checked in the handler).

---

## The Memory Model

Shared memory lives in named **pipes**. Each stored item is a `MemoryChunk`:

```rust
pub struct MemoryChunk {
    pub id: u64,                    // unique, monotonically assigned
    pub text: Arc<String>,          // the "probe": what is embedded and BM25-indexed
    pub context_payload: Arc<String>, // what is returned to agents and injected into prompts
    pub vector: Arc<Vec<f32>>,      // unit-normalized embedding of `text`
    pub source_app: String,
    pub timestamp: u64,
    pub category: MemoryCategory,
}
```

### Memory Tiers (`MemoryCategory`)

A one-byte tag on every chunk, set at write time and filterable at search time:

| Category | Meaning |
|---|---|
| `fact` | Episodic truths, logs, observations |
| `skill` | Procedures, code routines, tool-usage rules |
| `directive` | Permanent constraints, persona rules, policies |
| `general` (default) | Unclassified |

The filter is checked before any vector math, so filtered-out chunks cost almost nothing.

### Decoupled Probe and Context

Small chunks embed precisely but lack context; large chunks keep context but blur the embedding. Ore stores both: `text` is the tight probe that is embedded and indexed, and `context_payload` is the wider passage handed back to the model. Most strategies set them equal; `sentence_window` separates them.

### Chunking Strategies (`chunk_text`)

| Strategy | Probe (`text`) | Context (`context_payload`) |
|---|---|---|
| `sliding_window` (default) | Fixed word window, advancing by `size − overlap` | Same as probe |
| `sentence_aware` | Whole sentences packed up to `size` words, with overlap | Same as probe |
| `sentence_window` | **One sentence** | That sentence plus neighbours added alternately before and after until ~`size` words |
| `paragraph` | Paragraphs, kept under `size` words | Same as probe |
| `exact_match` | The whole text | Same as probe |

Defaults from `/ipc/share`: `size = 100`, `overlap = 20` (an overlap ≥ size becomes `size / 4`).

---

## Writing Knowledge (`POST /ipc/share`)

1. The handler checks `allowed_semantic_pipes`, chunks the text, and embeds each probe in one batch under the embedder lock (cache hits skip embedding).
2. `write_chunk` normalizes the vector to unit length, assigns an id, caches the embedding, and calls `insert_chunk_to_pipe`.
3. **The pipe is loaded from disk first** (`ensure_pipe_loaded`), so a write after a restart appends to the stored pipe instead of starting an empty one.
4. The chunk goes into the pipe according to its mode (below), and `persist_pipe` writes it to SSD if the pipe is persistent.

---

## Storage Modes (`PipeState`)

```rust
pub enum PipeState {
    Active { .. },             // Mode A: full-precision vectors in RAM
    ActivePqRecompute { .. },  // Mode B: product-quantized codes + re-embedding
    ActiveSq8Nvme { .. },      // Mode C: int8 vectors in RAM, f32 vectors on disk (mmap)
    Branched { .. },           // Copy-on-write branch of another pipe
    PagedToDisk,               // Evicted from RAM by GC; restored on next access
}
```

| | Mode A: `hot_simd` | Mode B: `pq_recompute` | Mode C: `sq8_nvme` |
|---|---|---|---|
| **RAM** | Full `f32` vectors | Product-quantized codes | `int8` vectors (¼ of `f32`) |
| **Disk** | `.pipe` | Not persisted | `.sq8pipe` (metadata) + `.f32vecs` (vectors, memory-mapped) |
| **Dense search** | Exact dot product over every chunk | Coarse PQ scan → top 32–64 candidates re-embedded and scored exactly | `int8` scan → top candidates re-scored with exact `f32` vectors from the mmap |
| **Capacity** | `max_hot_chunks` (default 10,000) | 50,000 | 1,000,000 |
| **Status** | Default | Experimental | Used automatically for large pipes |

### Choosing a Mode

The mode is fixed when a pipe is **created**, from the writing agent's manifest `[rag].vector_mode`:

- `auto` (default) and `hot_simd`: start in Mode A. When the pipe reaches `max_hot_chunks`, it **migrates to Mode C**: all vectors are written to a fresh `.f32vecs` file in one batch, quantized to `int8`, and the BM25 index is kept. (`hot_simd` currently behaves the same as `auto`.)
- `sq8_nvme`: start directly in Mode C.
- `pq_recompute`: start in Mode B.

### Mode B Notes

Mode B's `PqCodebook` supports mini-batch k-means training (`train_mini_batch`), but pipes are created with the untrained initial codebook, so the coarse candidate selection is approximate. Mode B pipes are also not persisted. Treat Mode B as experimental.

### Mode C Notes

- `MmapVectorStore` appends vectors to `<pipe>.f32vecs` and reads them through a memory map with no per-query allocation. Appends of the wrong dimension are rejected (`InvalidInput`).
- The vector dimension is stored with the pipe, so any embedder size (384 for all-MiniLM, 768 for Nomic, …) survives restarts.
- Mode C is a linear `int8` scan plus exact re-ranking, not a graph index. It is fast for hundreds of thousands of chunks; very large pipes scale linearly.

---

## Hybrid Search (`POST /ipc/search`)

```rust
pub fn search_pipe(&self, pipe_name, query_vector, query_text, top_k,
                   filter_app, filter_category, allow_time_decay, enable_hybrid) -> SearchOutcome

pub struct SearchOutcome {
    pub results: Vec<(f32, Arc<MemoryChunk>)>,
    pub top_dense_score: f32,   // best dense similarity
    pub top_fused_score: f32,   // best normalized RRF score
}
```

`search_pipe_recompute` is the async variant that re-embeds Mode B candidates; the handlers use it.

1. **Page-fault recovery.** The pipe is loaded from disk if needed.
2. **Filters.** `filter_app` and `filter_category` are applied before scoring.
3. **Dense pass.** Per the pipe's mode (table above). Time decay, if `allow_time_decay`: `score × clamp(1 − 0.01 × hours_old, 0.5, 1.0)`.
4. **Sparse pass (BM25).** Standard BM25 (`k1 = 1.2`, `b = 0.75`, smoothed IDF) over the probe text. The tokenizer keeps `_` and `-`, so identifiers and error codes (`ERR_0x99A`, `my_function`) match exactly.
5. **Reciprocal Rank Fusion.** `score = Σ 1 / (60 + rank)` over the dense and sparse rankings, then **normalized to [0, 1]** by dividing by the best possible score for the number of lists that returned results. A chunk ranked first in every active list scores 1.0, so thresholds mean the same thing whether one or both lists matched.

Each pool keeps `max(top_k, clamp(4 × top_k, 32, 200))` candidates. Ties are broken by chunk id, so results are deterministic. With `enable_hybrid = false`, the dense ranking is returned directly (scores clamped to [0, 1]).

### Search Request Overrides

`/ipc/search` accepts per-request overrides of the agent's `[rag]` settings: `enable_hybrid`, `enable_hyde`, `confidence_threshold`, `crag_fallback_tool`. Responses include each chunk's `context_payload`, `category`, and `is_crag_fallback`.

---

## Autonomous RAG (HyDE & CRAG)

Used by `/ipc/search` and, when `[rag].auto_rag = true`, by `/infer` and `/run` before inference:

- **HyDE.** The model writes a one-sentence hypothetical answer, and that is embedded instead of the raw question (answers sit closer to stored answers in vector space). It runs under its own identity (`<app_id>::hyde`) with a GPU lease, so it never touches the agent's KV cache.
- **CRAG.** If `max(top_fused_score, top_dense_score)` is below `confidence_threshold`, the configured WASM tool runs with the query as `argv[1]` (and on stdin). Successful output is embedded and stored in the pipe as a `fact` from that tool, then the search runs again. Output containing `KERNEL ERROR` is never stored.
- **Firewall.** Before injection into a prompt, every retrieved chunk passes the context firewall under the reading agent's manifest: chunks that trip the injection rules are dropped, and PII is redacted when enforced. See [Security Model](../security-model.md).

---

## Copy-on-Write Branches

Branches let an agent stage memory changes without touching a shared pipe until they are reviewed.

| Operation | Route | Behaviour |
|---|---|---|
| **Branch** | `POST /ipc/pipe/branch` | Creates `Branched { base_pipe, delta_chunks, bm25, created_at }`. Base chunks are shared by `Arc`, not copied; the branch gets its own copy of the base's BM25 index. |
| **Write** | `POST /ipc/share` to the branch | Appends to `delta_chunks` only. The base is untouched. |
| **Search** | `POST /ipc/search` on the branch | Sees base + deltas. |
| **Diff** | `POST /ipc/pipe/diff` | Returns the staged (unmerged) chunks. |
| **Merge** | `POST /ipc/pipe/merge` | Appends the deltas to the base (Mode A or Mode C) and persists it. |

Merge is safe under failure:

- Everything is validated (the branch belongs to that base, the base is in a mergeable state) **before** the deltas are drained.
- If the base changes state between validation and the merge, the deltas are restored to the branch and an error is returned.
- After a merge, the branch's deltas are empty, so the branch view never shows a chunk twice. If the branch is kept (`delete_branch_after_merge = false`), its BM25 index is resynchronized to the new base.
- Disk writes happen after the DashMap lock is released.

Branch and merge require the agent to be allowed on **both** the base and the branch pipe; diff requires the branch pipe. Patterns such as `"kb:branch/*"` in `allowed_semantic_pipes` grant access to all branches of `kb`.

Branches live in RAM only: they survive GC until `pipe_ttl_hours` after creation (or forever while they hold deltas), but not a kernel restart.

---

## RAPTOR Summary Trees (`POST /ipc/pipe/raptor_build`)

Builds a hierarchy of summaries over a pipe, for broad questions that span many chunks.

1. Existing RAPTOR summaries (`source_app` starting with `raptor_l`) are excluded from the leaves.
2. Chunks are greedily clustered by cosine similarity (> 0.40), `cluster_size` per cluster (default 4, clamped to 2–10).
3. Each cluster is summarized by the agent's default model, under the identity `<app_id>::raptor` with a GPU lease per summary.
4. Each summary is stored as `[RAPTOR-L<level>] …` (`fact`, `source_app = raptor_l<level>`), with the **normalized centroid** of its cluster's vectors as its embedding.
5. The summaries become the input of the next level, up to `max_levels` (default 2).

The request returns `{ target_pipe, generated_summaries, tree_levels }` once the tree is built. Re-running it adds a new set of summaries; earlier ones are not replaced.

---

## Persistence

| File (in `memory/`) | Mode | Contents |
|---|---|---|
| `<pipe>.pipe` | A | Chunks, `max_hot_chunks`, `is_persistent` |
| `<pipe>.sq8pipe` | C | `int8` chunk metadata, `vector_dim`, `max_chunks`, `is_persistent` |
| `<pipe>.f32vecs` | C | Raw `f32` vectors, appended; memory-mapped |

- **When:** after each `/ipc/share` to a persistent pipe (`persist_pipe`, mode-aware, branches skipped), after a merge, and at each hourly GC.
- **Crash-safe writes:** data goes to a uniquely named temp file, which is `fsync`ed, renamed over the target, and then the directory is synced.
- **Loading:** `ensure_pipe_loaded` prefers `.sq8pipe`, then `.pipe`. The loaded state is inserted only if the slot is still empty or `PagedToDisk`, so a concurrent first access can't overwrite another request's write.
- **Corruption:** a file that fails to deserialize is renamed to `<file>.corrupt-<timestamp>` and logged as `[CRITICAL]`, instead of being overwritten.

Pipes written by older Ore versions use a different on-disk layout and are quarantined (not loaded) after upgrading.

---

## Garbage Collection

`run_garbage_collection` runs hourly (see [Configuration](../configuration.md#memory-section)):

1. **Embedding cache:** entries older than `cache_ttl_hours` are evicted.
2. **Persistent pipes (Mode A and C):** written to disk and set to `PagedToDisk`, freeing RAM; the next access restores them.
3. **Ephemeral pipes and Mode B:** chunks older than `pipe_ttl_hours` are evicted (and removed from BM25); empty pipes are dropped.
4. **Branches:** expired deltas are evicted; an empty branch is kept until `pipe_ttl_hours` after its creation.

A TTL of `0` disables that sweep.

---

## Rate Limiter

```rust
pub fn check_and_add(&self, app_id: &str, limit: u32, requested_tokens: u32) -> bool
```

A per-agent counter in a 60-second window, limited by `[resources].max_tokens_per_minute`. `/run` and `/infer` each charge 1,000 tokens per request.

---

## Tests

`ore-core/src/ipc/mod.rs` covers, among others: the dot product, message-bus backpressure, all chunking strategies, BM25 keyword matching and eviction, category filtering, RRF normalization, CoW branch isolation / diff / merge, GC keeping fresh branches, the mmap store, Mode A → C migration, Mode B recompute, and restart durability for both modes including corrupt-file quarantine (`cargo test -p ore-core ipc`).

End to end, [`examples/ipc_mmu/`](../../examples/ipc_mmu/) runs the same features through the HTTP API with six cooperating agents (coordinator, researcher, two workers, reviewer, analyst), and includes HTTP benchmarks (`python run_suite.py --bench`). In-process benchmarks of the search core, without HTTP or the embedder, are in `ore-core/benches/cognitive_mmu.rs` (`cargo bench -p ore-core --bench cognitive_mmu`).

---

## Design Decisions

- **Normalize once, at write time.** Dense search is a pure dot product.
- **Two retrievers, rank-fused.** BM25 catches exact identifiers that embeddings blur; RRF combines them without calibrating their score scales.
- **Branches before merges.** Agents stage memory in isolation; shared pipes change only on an explicit, permission-checked merge.
- **Load before write, quarantine before overwrite.** Persistent memory is never silently replaced.
- **Bounded everything.** Inboxes, pipes, and caches all have explicit caps.

---

**← Back to:** [Kernel Internals Index](./README.md)
