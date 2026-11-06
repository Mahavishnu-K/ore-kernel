# 07 - Cognitive MMU Suite

> A multi-agent system that uses the Ore kernel as its memory, message broker, and access-control layer, together with tests and benchmarks for every Cognitive MMU feature.

Most multi-agent stacks assemble memory from separate services:
- a vector database for retrieval;
- a broker for messages;
- glue code for permissions, with every agent holding the same database credentials.

In Ore, all three live in the kernel:

- **Semantic Bus:** shared vector + BM25 memory.
- **Message Bus:** bounded agent-to-agent queues.
- **The agent's manifest:** the access-control list, checked on every call.

This suite runs six Python agents against those primitives the way a production swarm would, then measures what it costs.

## What it covers

| ID | Scenario | What it proves |
|---|---|---|
| `s01_share_search` | Share → search round trip | Scores normalized to [0, 1], `top_k`, sorted and deterministic results, every response field |
| `s02_hybrid_bm25` | Hybrid vs dense-only recall@3 | BM25 + RRF finds exact identifiers (`ERR_7731`, `pg-orders-07`) that embeddings blur, without hurting paraphrase queries |
| `s03_memory_tiers` | Memory tiers | `fact` / `skill` / `directive` tags survive storage; `filter_category` never leaks another tier |
| `s04_sentence_window` | Sentence-window chunking | The match is one precise sentence; `context_payload` carries its neighbours |
| `s05_cow_branches` | Copy-on-write branches | Branch isolation, `diff`, `merge`, merging twice is a no-op, no duplicates, `delete_branch_after_merge` |
| `s06_permissions` | Kernel-enforced isolation | `401` for unknown agents, `403` for denied pipes, refused writes never land, workers can't merge, message targets are allowlisted |
| `s07_message_bus` | Message Bus | FIFO order, round-trip latency, the 1,024-message backpressure limit, recovery after draining |
| `s08_swarm` | Production workflow | Research → branch work in parallel threads → review → merge; a compromised worker's branch is rejected |
| `s09_persistence` | Restart durability | Persistent pipes survive a kernel restart with no loss or duplication (two phases) |
| `s10_storage_modes` | Mode A → Mode C | Crossing `max_hot_chunks` converts the pipe to SQ8 + mmap; recall and exact-ID search keep working |
| `s11_hyde` / `s11_raptor` / `s11_auto_rag` | LLM features | HyDE search, a RAPTOR summary tree, and `POST /infer` answering from shared memory (skipped without `qwen2.5:0.5b`) |
| `s11_crag` | CRAG fallback | Low confidence runs a sandboxed WASM tool and stores its findings as facts (needs `--crag-tool`) |
| `s12_net_firewall` | `/net/fetch` egress rules | The kernel refuses domains, paths, and methods outside the manifest, as well as the kernel's own API, cloud metadata addresses, `file://` URLs, agents without `[network]`, and unknown agents. No internet needed |
| `s12_web_search` | Web search + agent-side CRAG | [`web_search.py`](./web_search.py) searches Wikipedia and DuckDuckGo through `/net/fetch`. A low-confidence question falls back to the web, the findings are stored as facts, and asking again is answered from memory. Skipped when offline |
| `s13_retrieval_eval` | Retrieval quality | Recall@1/3/5/10, MRR, and nDCG@10 for every query kind, dense vs hybrid, plus the 15 hardest queries. Hybrid search must find exact and technical IDs; CJK recall is a known gap |
| `s14_crag_calibration` | CRAG threshold calibration | Score distributions, histograms, and AUC for answerable vs unanswerable queries, with a suggested threshold per confidence signal. Shows that the kernel's current gate rarely falls back (known gap) |
| `s15_chunking` | Chunking on long documents | All four strategies on ten long documents: chunk counts, probe vs payload size, whether the answer is in the matched sentence or only in its context. Includes the CJK sentence-split gap |
| `s16_firewall_rag` | Auto-RAG context firewall | PII planted in memory never reaches the answer; an injected chunk is dropped (its canary word never appears); a reworded injection that slips through is a known gap. Needs the model |

### The swarm (`s08`)

```
                 Message Bus                               Semantic Bus
 coordinator ──task──▶ researcher ──share (fact/skill/directive)──▶ mmu_facts_*
     │                                                                    │ search
     ├──task──▶ worker_a ──writes findings──▶ mmu_br_*_a ◀── branch ──┐  │
     ├──task──▶ worker_b ──findings + injection──▶ mmu_br_*_b ◀───────┤  │
     │              │ review_request                                 │  │
     │              ▼                                                │  │
     ◀──verdict── reviewer ── diff → policy check → merge ──▶ mmu_kb_*  (reviewer owns it)
                                                                   │
                                          analyst ── search ───────┘
```

Workers can read `mmu_facts_*` and write only to `mmu_br_*`. Their manifests don't list `mmu_kb_*`, so the kernel refuses any direct write to the knowledge base. The reviewer holds both prefixes, which branching and merging require.

`worker_b` slips a directive-tier prompt injection into its branch. The reviewer sees it in the diff, rejects the branch, and none of it reaches `mmu_kb_*`.

## Setup

**Requirements:**
- Python 3.8+ with `requests`.
- The kernel running.
- `ore pull system-embedder`.
- Optional: `ore pull qwen2.5:0.5b`, for the `s11` LLM scenarios.

```bash
cd examples/ipc_mmu

# 1. Install the 13 suite manifests into <ore_dir>/manifests (overwrites mmu_*.toml only)
python run_suite.py --install-manifests

# 2. Restart the kernel: manifests are read at boot
cargo run --release -p ore-server

# 3. Run everything
python run_suite.py
```

## Running

```bash
python run_suite.py                           # all scenarios
python run_suite.py --only s05,s08 -v         # a subset, with per-step detail
python run_suite.py --skip-llm                # skip HyDE / RAPTOR / Auto-RAG
python run_suite.py --only s11_crag --crag-tool <tool>   # CRAG with a WASM tool from tools/
python run_suite.py --only s12 -v             # /net/fetch firewall + web search (needs internet)
python run_suite.py --only s13,s14,s15 -v     # retrieval metrics, CRAG calibration, chunking (offline)

# Web search on its own, as the mmu_web agent
python web_search.py "Rust programming language"
python web_search.py --json --max 3 --provider wikipedia "Dennis Ritchie"
python web_search.py --ingest mmu_web_notes "WebAssembly System Interface"

# Restart durability (two phases)
python run_suite.py --only s09 --phase write
#   ...restart the kernel...
python run_suite.py --only s09 --phase verify

# Benchmarks (instead of the scenarios)
python run_suite.py --bench
python run_suite.py --bench --sizes 1000,5000 --concurrency 1,8 --iterations 500
```

- **Exit status:** `1` if any scenario fails; `0` otherwise. SKIPs don't fail a run.
- **Reports:** every run writes `results/run_<id>.json` and `results/run_<id>.md`. They record the kernel version, the engine and device, the installed models, and the arguments.

### Status meanings

| Status | Meaning |
|---|---|
| PASS | Every check held |
| FAIL | A check failed; the run exits with status 1 |
| XFAIL | A documented limitation was hit (listed in the detail). Doesn't fail the run; once fixed, the scenario passes |
| SKIP | Needs something missing: the model, internet, or a two-phase step |

### Isolation and cleanup

- **Pipe names:** every run puts a run id in its pipe names (`mmu_kb_<run>_s05`), so runs never see each other's data.
- **Persistence:** pipes use `semantic_persistence = false` and expire through the kernel's hourly GC. The exceptions are `mmu_archivist`'s pipe (`mmu_archive_s09`) and Mode C's `.f32vecs` files, which do land in `memory/`.
- **Removing those files:** `python run_suite.py --cleanup` lists them and asks before deleting (`--yes` skips the question).

## The corpus

All data is in [`corpus/`](./corpus/): deterministic and offline. It runs in a few minutes on a CPU laptop, dominated by embedding about 500 chunks.

| Module | Documents | Queries | What it tests |
|---|---:|---:|---|
| `core.py` | 24 | 16 | Operations-team facts, skills, directives; exact IDs and paraphrases. The behaviour scenarios (s01-s08) use only this set |
| `near_miss.py` | 20 | 20 | Look-alikes: `INC-4471` vs `INC-4417`, `FF_BULK_EXPORT` vs `FF_BULK_IMPORT`, SSH vs TLS rotation. Ranking precision |
| `multi_doc.py` | 6 | 12 | Questions answered by 2-3 documents; recall@k counts how many were found |
| `technical.py` | 20 | 18 | Stack traces, IP:port, versions, hex addresses, hashes, UUIDs, config keys. The BM25 tokenizer keeps `_` and `-` but splits on `.` `:` `/` |
| `long_docs.py` | 10 (chunked) | 32 | Postmortems, runbooks, a design doc, a capacity review (~4,700 words, 105 paragraphs); answers judged by phrase in the returned text or payload |
| `cjk.py` | 9 | 9 | Chinese, Japanese, Korean, French, Russian, mixed scripts |
| `unanswerable.py` | 0 | 25 | 7 off-domain and 18 near-domain questions with no answer; the negatives for CRAG calibration |
| `adversarial.json` | 26 cases | | Injections, evasions, PII and secrets, false positives (firewall) |

That's 115 documents (about 6,200 words for retrieval) and 132 queries. Labels are checked at import: a query pointing at a missing document, or an answer phrase that doesn't occur exactly once, stops the suite.

**Metrics** (`metrics.py`, answerable queries, binary relevance):
- **Recall@k:** share of a query's relevant documents in the top k.
- **MRR:** mean of 1 / rank of the first relevant result.
- **nDCG@10:** rank-weighted gain against the ideal order.
- **Calibration:** for each confidence signal (dense cosine, hybrid fused score, and the kernel's `max(fused, dense)` gate):
  - score distributions for answerable vs unanswerable queries;
  - **AUC**, the chance an answerable query outscores an unanswerable one;
  - the threshold that maximizes catch rate minus false-fallback rate.

**Firewall, offline:** the same adversarial cases run through the real `ContextFirewall` in a Rust test:

```bash
cargo test -p ore-core --test firewall_corpus -- --nocapture
```

`drop`, `redact`, and `pass` cases are asserted; known evasions and false positives are reported, and flagged when their behaviour changes.

## Benchmarks

Published results from this suite (Ryzen 7 5800HS, CPU only), with analysis and the known gaps, are in [docs/benchmarks.md](../../docs/benchmarks.md).

`--bench` measures what an agent process actually waits for: an HTTP request on localhost, the manifest check, and JSON encoding.

Search queries are embedded during warm-up and then served from the kernel's embedding cache, so the timed searches measure retrieval, not the embedder. Share throughput is reported both cold (embedder) and warm (cache).

| Benchmark | Measures |
|---|---|
| `share_throughput` | Chunks/s through `/ipc/share`, cold vs from the embedding cache |
| `search_latency` | p50/p95/p99 by pipe size (default 1k / 5k / 10k), hybrid on and off |
| `search_filtered` | Search restricted to the `skill` tier (25% of the chunks) |
| `search_concurrency` | Total throughput with 1 / 4 / 16 concurrent searchers |
| `retrieval` | Recall@1/5/10, MRR, and nDCG@10 per query kind on the labelled corpus, dense vs hybrid, plus per-query latency for the whole set |
| `branch_create` | Branch creation cost as the base grows (chunks are shared; the BM25 index is copied) |
| `diff`, `merge` | Reviewing and committing a 100-chunk branch |
| `mode_search` | Mode A (f32 in RAM) vs Mode C (SQ8 + mmap) at 2,000 chunks |
| `msgbus_rtt` | Agent-to-agent send + receive, with no separate broker process |

**Notes:**
- **Benchmark manifest:** benchmarks run as `mmu_bench`, whose `max_hot_chunks = 20000` keeps the 10k pipe in Mode A. Sizes above 20,000 migrate to Mode C.
- **In-process numbers:** to see the search core without HTTP or the embedder, use the Rust benchmarks:

  ```bash
  cargo bench -p ore-core --bench cognitive_mmu            # full run, report in target/criterion/
  cargo bench -p ore-core --bench cognitive_mmu -- --quick # fast pass
  ```

  They cover:
  - the dot product, f32 vs int8 (384 and 768 dims);
  - `search_pipe` dense and hybrid at 1k / 5k / 10k chunks;
  - category filtering;
  - BM25 build and scoring;
  - branch creation;
  - Mode A vs Mode C search;
  - every chunking strategy.

## Files

| File | Purpose |
|---|---|
| `run_suite.py` | Command line: scenarios, benchmarks, reports, manifest install, cleanup |
| `mmu_client.py` | `OreMmuClient`: typed wrappers for every MMU route (extends `../ore_client.py`) |
| `agents.py` | Agent roles: `Coordinator`, `Researcher`, `Worker`, `Reviewer`, `Analyst` |
| `corpus/` | Labelled evaluation corpus (see [The corpus](#the-corpus)) |
| `evaluation.py` | Builds the evaluation index and runs every query dense and hybrid; shared by s13, s14, and `--bench` |
| `metrics.py` | Recall@k, MRR, nDCG@10, calibration (AUC, suggested threshold), text histograms |
| `harness.py` | Scenario registry, PASS/FAIL/SKIP, percentiles, report writer |
| `bench_mmu.py` | HTTP benchmarks |
| `web_search.py` | Web search through `POST /net/fetch` (Wikipedia + DuckDuckGo, no API keys), `crag_search()` for agent-side CRAG, and a command line |
| `scenarios/` | One module per scenario |
| `manifests/` | One manifest per agent (below) |

### Manifests

| Agent | Pipes | Message targets | Notes |
|---|---|---|---|
| `mmu_coordinator` | none | the researcher, both workers, the reviewer | Owns no memory |
| `mmu_researcher` | `mmu_facts_*` | the coordinator | |
| `mmu_worker_a`, `mmu_worker_b` | `mmu_br_*`, `mmu_facts_*` | the coordinator, the reviewer | No access to `mmu_kb_*` |
| `mmu_reviewer` | `mmu_kb_*`, `mmu_br_*` | the coordinator, both workers | Creates, diffs, and merges branches |
| `mmu_analyst` | `mmu_rag_kb`, `mmu_kb_*`, `mmu_facts_*` | the coordinator | `auto_rag = true`; `qwen2.5:0.5b` |
| `mmu_bulk` | `mmu_bulk_*` | none | `max_hot_chunks = 256`, so Mode C kicks in early |
| `mmu_archivist` | `mmu_archive_*` | none | `semantic_persistence = true` |
| `mmu_outsider` | `other_*` | none | Negative tests |
| `mmu_bench` | `mmu_bench_*` | none | `max_hot_chunks = 20000` |
| `mmu_crag` | `mmu_crag_*` | none | `allowed_tools = ["*"]`, no network; narrow it if you keep it installed |
| `mmu_web` | `mmu_web_*` | none | Network limited to `GET en.wikipedia.org/w/api.php` and `GET api.duckduckgo.com/` |
| `mmu_guard` | `mmu_guard_kb` | none | `auto_rag = true`, `enforce_pii_redaction = true`; `qwen2.5:0.5b` |
