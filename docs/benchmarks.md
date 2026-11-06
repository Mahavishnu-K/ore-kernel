# Benchmarks: Cognitive MMU

Measured results for Ore's shared agent memory: the Semantic Bus, hybrid search, copy-on-write (CoW) branches, storage modes, the Message Bus, and retrieval quality on a labelled corpus.

There are two sources:

| Source | What it measures | Command |
|---|---|---|
| [`examples/ipc_mmu/`](../examples/ipc_mmu/) | End to end over HTTP, the way an agent sees it: request, manifest check, search, JSON. Also retrieval quality and the functional scenarios | `python run_suite.py` / `--bench` |
| [`ore-core/benches/cognitive_mmu.rs`](../ore-core/benches/cognitive_mmu.rs) | The search core in-process with `criterion`: no HTTP, no embedder | `cargo bench -p ore-core --bench cognitive_mmu` |

## Test machine

| | |
|---|---|
| CPU | AMD Ryzen 7 5800HS (8 cores / 16 threads), laptop |
| RAM | 16 GB |
| GPU | None used: CPU only, `Native Candle Engine`, device `Host CPU (SIMD)` |
| OS | Windows 11 (build 26200) |
| Toolchain | rustc 1.93.0 (release profile), Python 3.10.11 |
| Embedder | `system-embedder` (nomic-embed-text-v1.5, 768 dims) |
| LLM (HyDE, RAPTOR, Auto-RAG) | `qwen2.5:0.5b` |
| Date | 2026-11-06 |

> [!NOTE]
> **Single-Machine Laptop Numbers:** CPU timings vary with the power plan, battery state and temperature; an earlier partial criterion run measured the dot product at about twice the times below. These benchmarks demonstrate ORE's architectural efficiency on standard consumer hardware. Dedicated server environments and discrete GPUs will yield significantly higher absolute throughput.

## Functional scenarios

`python run_suite.py -v`, then the two-phase persistence test (`--phase write`, kernel restart, `--phase verify`).

**16 pass, 3 known gaps, 1 not run.**

| Scenario | Status | Time |
|---|---|---:|
| s01 Share → search round trip, scores, `top_k`, determinism | PASS | 1.8 s |
| s02 Hybrid (BM25 + dense, RRF) vs dense-only recall@3 | PASS | 1.0 s |
| s03 Memory tiers: category tags and `filter_category` | PASS | 0.1 s |
| s04 Sentence-window: precise probe, wide `context_payload` | PASS | 0.6 s |
| s05 CoW branches: branch → write → diff → merge, no duplicates | PASS | 0.9 s |
| s06 Manifest isolation: 401 / 403 / denied writes / denied messages | PASS | 0.3 s |
| s07 Message Bus: FIFO order, RTT, 1,024-message backpressure | PASS | 2.4 s |
| s08 Multi-agent swarm: research → branch work → review → merge | PASS | 2.0 s |
| s09 Persistence: chunks survive a kernel restart | PASS | 3.6 s + 1.8 s |
| s10 Mode A → Mode C (SQ8 + mmap) migration keeps recall | PASS | 28.3 s |
| s11 HyDE: search with a hypothetical answer | PASS | 7.1 s |
| s11 RAPTOR: build a summary tree over a pipe | PASS | 140.1 s |
| s11 Auto-RAG: `POST /infer` answers from shared memory | PASS | 16.1 s |
| s11 CRAG: low confidence triggers a sandboxed fallback tool | not run | needs `--crag-tool` |
| s12 `/net/fetch`: manifest egress rules refuse what isn't allowed | PASS | 0.02 s |
| s12 Web search via `/net/fetch` + agent-side CRAG into memory | PASS | 2.6 s |
| s13 Retrieval quality: Recall@k, MRR, nDCG@10 by query kind | PASS | 26.1 s |
| s14 CRAG gate calibration | known gap | |
| s15 Chunking strategies on long documents | known gap | 29.7 s |
| s16 Auto-RAG firewall: PII redacted, injected chunks dropped | known gap | 40.1 s |

The long scenarios are dominated by CPU inference: embedding (s10, s13, s15) and LLM generation (s11, s16).

**Known gaps** (the scenario keeps running and reports XFAIL; the check turns into a PASS once the gap is fixed):

- **s14, CRAG gate:** the kernel's gate uses `max(fused, dense)`, and normalized RRF keeps the fused top score between 0.5 and 1.0. At the default threshold of 0.70 it catches **0%** of unanswerable queries; dense similarity alone would catch 84%. See [CRAG calibration](#crag-threshold-calibration).
- **s15, CJK sentences:** `sentence_window` splits only on `.`, `!` or `?` followed by a space, so six Chinese sentences became one chunk.
- **s16, firewall:** a reworded injection ("ignore **the** previous instructions") passed the Context Firewall and the model obeyed it. PII redaction and the other injection drops held.

## Search core (in-process, criterion)

Mean time per operation. Vectors are random unit vectors; text is synthetic records. `hybrid` = BM25 + dense with RRF fusion; all searches use `top_k = 5`.

### Search by pipe size

| Chunks | Dense, 384-d | Hybrid, 384-d | Dense, 768-d | Hybrid, 768-d |
|---:|---:|---:|---:|---:|
| 1,000 | 0.09 ms | 0.19 ms | 0.14 ms | 0.24 ms |
| 5,000 | 0.48 ms | 1.11 ms | 1.39 ms | 1.82 ms |
| 10,000 | 1.64 ms | 2.66 ms | 3.14 ms | 3.94 ms |

The 10,000-chunk pipe is Mode A's default capacity. Search is a brute-force scan, so time grows linearly with the pipe; BM25 adds 0.1–1.0 ms on top.

### Other operations

| Operation | Size | Time |
|---|---|---:|
| Dot product, f32 | 384 / 768 dims | 52.1 ns / 101.5 ns |
| Dot product, SQ8 int8 | 384 / 768 dims | 53.8 ns / 103.1 ns |
| Hybrid search, no filter | 10,000 chunks, 384-d | 1.93 ms |
| Hybrid search, `filter_category = skill` (25% of chunks) | 10,000 chunks, 384-d | 0.74 ms |
| BM25 index build | 10,000 docs | 26.9 ms |
| BM25 query | 10,000 docs | 0.13 ms |
| CoW branch creation | 1,000-chunk base | 0.21 ms |
| CoW branch creation | 10,000-chunk base | 2.72 ms |
| Hybrid search, Mode A (f32 in RAM) | 5,000 chunks, 384-d | 0.58 ms |
| Hybrid search, Mode C (SQ8 + mmap) | 5,000 chunks, 384-d | 1.26 ms |
| Chunking a 10 KB document: `paragraph` | | 21 µs |
| Chunking a 10 KB document: `sliding_window` | | 32 µs |
| Chunking a 10 KB document: `sentence_aware` | | 47 µs |
| Chunking a 10 KB document: `sentence_window` | | 58 µs |

**What these show:**

- **The category filter runs before scoring.** Restricting the search to a quarter of the chunks cuts the time to 39%.
- **SQ8 is about memory, not speed.** The int8 dot product is no faster than f32 at these sizes, and Mode C search is about 2× slower than Mode A. What Mode C buys is a 4× smaller working set and vectors that live on disk.
- **Branch creation is not constant-time.** Chunks are shared by reference, but the BM25 index is copied, so a branch of a 10× larger base costs about 13× more. It is still under 3 ms at Mode A's capacity.

**Run-to-run variance:** a full run 20 minutes earlier, with the same code, was 2–38% slower on every benchmark. Treat differences under about 30% between two runs as noise on this laptop.

## End to end over HTTP

`python run_suite.py --bench --sizes 1000,5000`, run as the `mmu_bench` agent. Every number includes a Python `requests` call to localhost, the manifest check and JSON encoding. Search queries are embedded once during warm-up, so search timings measure retrieval, not the embedder.

### Search latency

| Benchmark | Pipe | p50 | p95 | p99 | Throughput |
|---|---:|---:|---:|---:|---:|
| Dense search | 1,000 | 4.3 ms | 6.2 ms | 7.1 ms | 218/s |
| Hybrid search | 1,000 | 4.6 ms | 5.5 ms | 6.3 ms | 221/s |
| Hybrid, `filter_category = skill` (25%) | 1,000 | 4.2 ms | 4.8 ms | 5.1 ms | 250/s |
| Dense search | 5,000 | 6.7 ms | 7.7 ms | 8.5 ms | 146/s |
| Hybrid search | 5,000 | 10.8 ms | 13.1 ms | 15.0 ms | 104/s |
| Hybrid, `filter_category = skill` (25%) | 5,000 | 8.3 ms | 9.4 ms | 9.8 ms | 154/s |

At 1,000 chunks the same search takes 0.2 ms in-process, so nearly all of the 4–5 ms is the HTTP round trip and the Python client.

### Concurrent agents

Hybrid search on a 5,000-chunk pipe, with N threads searching at once:

| Searchers | p50 | p95 | Total throughput |
|---:|---:|---:|---:|
| 1 | 10.8 ms | 12.6 ms | 102 searches/s |
| 4 | 14.1 ms | 25.3 ms | 260 searches/s |
| 16 | 35.3 ms | 55.7 ms | 389 searches/s |

Pipes are read concurrently (no global lock), so 16 searchers get 3.8× the throughput of one.

### Writes, branches and messages

| Benchmark | Params | p50 | Mean | Throughput |
|---|---|---:|---:|---:|
| Share, cold (runs the embedder) | 1,000 chunks | | 78.5 ms/chunk | **12.7 chunks/s** |
| Share, warm (embedding cache) | 1,000 chunks | | 0.03 ms/chunk | **31,083 chunks/s** |
| Branch creation | 1,000-chunk base | 3.4 ms | 3.4 ms | 290/s |
| Branch creation | 5,000-chunk base | 7.2 ms | 7.2 ms | 139/s |
| Diff | 100 staged chunks | 3.0 ms | 3.2 ms | 317/s |
| Merge (and delete the branch) | 100 staged chunks | 4.1 ms | 4.2 ms | 239/s |
| Hybrid search, Mode A (f32 in RAM) | 2,000 chunks | 5.9 ms | 5.4 ms | 184/s |
| Hybrid search, Mode C (SQ8 + mmap) | 2,000 chunks | 7.1 ms | 6.2 ms | 162/s |
| Message Bus round trip (send + listen) | | 4.7 ms | 4.8 ms | 210/s |

**The embedder is the bottleneck for writes.** A cold share of 1,000 chunks takes about 80 s on this CPU (768-dim Nomic, batched forward passes). Re-sharing text the kernel has already embedded is about 2,400× faster. Everything else (search, branches, merges, messages) runs in milliseconds.

The Message Bus round trip is two HTTP calls, with no broker process to run.

## Retrieval quality

[s13](../examples/ipc_mmu/scenarios/s13_retrieval_eval.py) indexes a labelled, fully offline corpus and scores 107 answerable queries. The index holds 79 whole documents plus 10 long documents split into sentence-window chunks.

Query kinds:
- **exact:** IDs and error codes.
- **semantic:** paraphrases.
- **near_miss:** similar documents that differ in one key detail.
- **multi_doc:** answers spread over several documents.
- **technical:** paths, versions and stack traces.
- **long:** questions about the long documents.
- **cjk:** Chinese.

The corpus is in [`examples/ipc_mmu/corpus/`](../examples/ipc_mmu/corpus/).

| Query kind | n | Dense R@1 | Hybrid R@1 | Dense R@5 | Hybrid R@5 | Dense MRR | Hybrid MRR | Dense nDCG@10 | Hybrid nDCG@10 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| exact | 8 | 0.25 | **0.88** | 0.88 | **1.00** | 0.542 | **0.938** | 0.656 | **0.954** |
| semantic | 8 | **0.88** | 0.50 | 1.00 | 1.00 | **0.938** | 0.698 | **0.954** | 0.774 |
| near_miss | 20 | 0.70 | **0.85** | **1.00** | 0.95 | 0.817 | **0.897** | 0.863 | **0.922** |
| multi_doc | 12 | **0.29** | 0.25 | **0.90** | 0.88 | **0.819** | 0.743 | **0.851** | 0.817 |
| technical | 18 | 0.94 | **1.00** | 0.94 | **1.00** | 0.954 | **1.000** | 0.964 | **1.000** |
| long | 32 | 0.62 | **0.69** | 0.88 | **0.94** | 0.743 | **0.788** | 0.791 | **0.839** |
| cjk | 9 | 0.78 | 0.78 | 0.78 | 0.78 | 0.796 | 0.796 | 0.817 | 0.817 |
| **all answerable** | **107** | 0.66 | **0.73** | 0.91 | **0.94** | 0.805 | **0.844** | 0.845 | **0.881** |

For multi_doc, R@k is the share of each query's relevant documents found in the top k.

**What this shows:**

- **Hybrid wins overall.** MRR goes from 0.805 to 0.844, and Recall@10 reaches 0.99.
- **BM25 is what finds exact identifiers.** For exact-ID queries, Recall@1 goes from 0.25 to 0.88; dense embeddings blur `ERR_7731` and `ERR_7713` together.
- **Hybrid costs some ranking on paraphrases.** For semantic queries, Recall@1 drops from 0.88 to 0.50, because BM25 favours documents that share surface words with the question. The right document is still in the top 5 for every semantic query.
- **CJK gains nothing from BM25.** The tokenizer splits only on non-alphanumeric characters, so an unspaced Chinese sentence becomes a single token that never matches. Those results are the dense ones.

### Chunking strategies

[s15](../examples/ipc_mmu/scenarios/s15_chunking.py) runs 32 questions over the 10 long documents and returns the top 5. The *probe* is the embedded text that was matched; the *payload* is the context the agent receives.

| Strategy | Params | Chunks | Probe words | Payload words | Answer in payload @1 | Answer in payload @5 | MRR |
|---|---|---:|---:|---:|---:|---:|---:|
| `sliding_window` | 80 / 16 overlap | 77 | 75.3 | 75.3 | 0.66 | 0.94 | 0.792 |
| `sentence_aware` | 80 / 16 overlap | 74 | 69.6 | 69.6 | 0.69 | 0.94 | 0.794 |
| `sentence_window` | 60 | 401 | 11.8 | 67.6 | 0.69 | **0.97** | 0.793 |
| `paragraph` | 120 | 52 | 90.9 | 90.9 | 0.69 | **0.97** | **0.812** |

`sentence_window` matches on a single sentence (12 words) but hands the agent the surrounding window (68 words). That is why the answer is in its payload at rank 1 more often than in its probe (0.69 vs 0.59).

## CRAG threshold calibration

[s14](../examples/ipc_mmu/scenarios/s14_crag_calibration.py) compares top scores for the 107 answerable queries with 25 unanswerable ones. 18 of the unanswerable queries are near-domain: plausible questions that share words with stored text.

| Confidence signal | AUC | Answerable p50 | Unanswerable p50 | Best threshold | Catch | False fallback | Catch @ 0.70 | False fallback @ 0.70 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Dense cosine | **0.832** | 0.754 | 0.658 | 0.70 | 84% | 28% | **84%** | 28% |
| Hybrid fused (RRF) | 0.750 | 1.0 | 0.984 | 0.99 | 60% | 15% | 0% | 0% |
| Kernel gate, `max(fused, dense)` | 0.750 | 1.0 | 0.984 | 0.99 | 60% | 15% | **0%** | 0% |
| Dense, near-domain negatives only | 0.769 | 0.754 | 0.667 | 0.70 | 78% | 28% | 78% | 28% |

*Catch* = unanswerable queries that fall back to the CRAG tool (wanted). *False fallback* = answerable queries that fall back (a wasted tool run).

Dense cosine separates answerable from unanswerable queries reasonably well, and 0.70 is its best threshold. The kernel gate takes `max(fused, dense)`, and normalized RRF puts nearly every query at 0.98–1.0. So the gate never fires at 0.70: this is the s14 known gap. Gating on dense similarity alone would fix it.

## Reproducing

```powershell
# Kernel (terminal 1)
cargo run --release -p ore-server

# Suite (terminal 2), from examples/ipc_mmu
python run_suite.py --install-manifests        # once, then restart the kernel
python run_suite.py -v                         # scenarios + retrieval/calibration/chunking reports
python run_suite.py --phase write --only s09   # restart the kernel, then:
python run_suite.py --phase verify --only s09
python run_suite.py --bench --sizes 1000,5000  # HTTP benchmarks

# In-process search core (from the repo root)
cargo bench -p ore-core --bench cognitive_mmu
```

Each suite run writes `examples/ipc_mmu/results/run_<id>.md` and `.json`. Criterion writes its HTML report to `target/criterion/report/index.html`. The numbers on this page come from runs `20261106_135442_ec13` (scenarios), `20261106_144630_d88f` (HTTP benchmarks) and `20261106_152840_7d4e` / `20261106_152915_1166` (s09). The criterion numbers are the means from the run of 2026-11-06, 16:25–16:29.
