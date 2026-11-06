"""
bench_mmu.py - End-to-end Cognitive MMU benchmarks over HTTP.

Every number includes the HTTP round trip, the manifest permission check, and
JSON encoding: this is what an agent process actually waits for. For
in-process numbers without HTTP, run:

    cargo bench -p ore-core --bench cognitive_mmu

Search queries are embedded once during warm-up and then hit the kernel's
embedding cache, so timed searches measure retrieval rather than the embedder.
Share throughput is measured separately, cold (embedder) and warm (cache).
"""

import random
import time
from concurrent.futures import ThreadPoolExecutor

import corpus
import evaluation
import metrics
from harness import sample, summarize, timed
from mmu_client import OreMmuClient
from scenarios.common import BENCH, BULK, COORDINATOR, WORKER_A

BULK_SEED = 7  # stable across runs, so re-filling a pipe is served from the embedding cache


def row(name: str, params: str, stats: dict, notes: str = "") -> dict:
    print(f"    {name:<20} {params:<32} p50={stats.get('p50_ms', '-'):>8} ms  ops/s={stats.get('ops_s', '-')}")
    return {"name": name, "params": params, "stats": stats, "notes": notes}


def fill(c, app: str, pipe: str, n: int, skill_every: int = 0) -> float:
    """Fills `pipe` with n synthetic docs; every `skill_every`-th is a skill. Returns seconds."""
    docs = corpus.synthetic_docs(n, seed=BULK_SEED)
    start = time.perf_counter()
    if skill_every:
        c.share_docs(app, pipe, [d for i, d in enumerate(docs) if i % skill_every], category="fact")
        c.share_docs(app, pipe, [d for i, d in enumerate(docs) if i % skill_every == 0], category="skill")
    else:
        c.share_docs(app, pipe, docs)
    return time.perf_counter() - start


def query_set(n_docs: int, count: int = 32) -> list:
    r = random.Random(11)
    docs = corpus.synthetic_docs(n_docs, seed=BULK_SEED)
    picks = r.sample(range(n_docs), min(count, n_docs))
    half = len(picks) // 2
    return [docs[i] for i in picks[:half]] + [f"R{i:06d}" for i in picks[half:]]


def run_benchmarks(ctx) -> list:
    c = ctx.client
    a = ctx.args
    sizes = [int(s) for s in a.sizes.split(",") if s]
    levels = [int(s) for s in a.concurrency.split(",") if s]
    it, warm = a.iterations, a.warmup
    out = []

    # ─── Share throughput: embedder vs cache ─────────────────────
    print("  share throughput")
    seed = int(ctx.run_id[-4:], 16) + 1000  # texts this kernel has never embedded
    docs = corpus.synthetic_docs(1000, seed=seed)
    cold_ms, _ = timed(lambda: c.share_docs(BENCH, ctx.pipe("mmu_bench", "tp_cold"), docs))
    warm_ms, _ = timed(lambda: c.share_docs(BENCH, ctx.pipe("mmu_bench", "tp_warm"), docs))
    for label, ms in (("cold (embedder)", cold_ms), ("warm (embedding cache)", warm_ms)):
        stats = {"n": len(docs), "mean_ms": round(ms / len(docs), 3), "ops_s": round(len(docs) / (ms / 1000), 1)}
        out.append(row("share_throughput", label, stats, "chunks/s; batched paragraph shares of up to 500 docs"))

    # ─── Search latency by pipe size ─────────────────────────────
    for n in sizes:
        pipe = ctx.pipe("mmu_bench", f"n{n}")
        fill_s = fill(c, BENCH, pipe, n, skill_every=4)
        print(f"  pipe {n} chunks (filled in {fill_s:.1f}s)")
        queries = query_set(n)
        for hybrid in (False, True):
            run = lambda i, h=hybrid: c.search(BENCH, pipe, queries[i % len(queries)], top_k=5, enable_hybrid=h)  # noqa: E731
            stats = summarize(sample(run, it, max(warm, len(queries))))
            out.append(row("search_latency", f"n={n} hybrid={'on' if hybrid else 'off'}", stats, "top_k=5"))
        run = lambda i: c.search(BENCH, pipe, queries[i % len(queries)], top_k=5, filter_category="skill")  # noqa: E731
        out.append(row("search_filtered", f"n={n} category=skill (25%)", summarize(sample(run, it, warm)), "hybrid on"))

    # ─── Concurrent searchers ────────────────────────────────────
    n = max(sizes)
    pipe = ctx.pipe("mmu_bench", f"n{n}")
    queries = query_set(n)
    for level in levels:
        clients = [OreMmuClient(c.base_url) for _ in range(level)]
        per = max(1, it // level)

        def worker(k):
            cl = clients[k]
            return sample(lambda i: cl.search(BENCH, pipe, queries[(i + k) % len(queries)], top_k=5), per)

        start = time.perf_counter()
        with ThreadPoolExecutor(level) as pool:
            samples = [s for chunk in pool.map(worker, range(level)) for s in chunk]
        wall = time.perf_counter() - start
        out.append(row("search_concurrency", f"n={n} threads={level}", summarize(samples, wall), "ops/s = total throughput"))

    # ─── Retrieval quality on the labelled corpus ────────────────
    print("  retrieval quality (labelled corpus)")
    run = evaluation.evaluate(c, BENCH, ctx.pipe("mmu_bench", "eval"))
    n_queries = len(corpus.EVAL_QUERIES)
    for mode in ("dense", "hybrid"):
        stats = {"n": n_queries, "mean_ms": round(run.query_s[mode] / n_queries * 1000, 3)}
        for r in metrics.by_kind(evaluation.scores(run, mode)):
            notes = f"R@1={r.recall[1]:.2f} R@5={r.recall[5]:.2f} R@10={r.recall[10]:.2f} MRR={r.mrr:.3f} nDCG@10={r.ndcg10:.3f}"
            out.append(row("retrieval", f"{mode}: {r.label} (n={r.n})", stats if r.label == "all answerable" else {"n": r.n}, notes))
    ctx.section(
        "Retrieval quality (benchmark index)",
        metrics.retrieval_table({m: metrics.by_kind(evaluation.scores(run, m)) for m in ("dense", "hybrid")}),
    )

    # ─── CoW branch creation vs base size ────────────────────────
    for n in sizes:
        base = ctx.pipe("mmu_bench", f"n{n}")
        run = lambda i, b=base, n=n: c.branch(BENCH, b, ctx.pipe("mmu_bench", f"n{n}_br{i}"))  # noqa: E731
        out.append(row("branch_create", f"base={n} chunks", summarize(sample(run, 20)), "chunks shared by Arc; BM25 index copied"))

    # ─── Diff + merge of a 100-chunk review ──────────────────────
    base = ctx.pipe("mmu_bench", "merge")
    fill(c, BENCH, base, 100)
    diffs, merges = [], []
    for round_ in range(5):
        branch = ctx.pipe("mmu_bench", f"merge_br{round_}")
        c.branch(BENCH, base, branch)
        c.share_docs(BENCH, branch, corpus.synthetic_docs(100, start=10_000 + round_ * 100, seed=BULK_SEED))
        ms, d = timed(lambda: c.diff(BENCH, base, branch))
        diffs.append(ms)
        ms, merged = timed(lambda: c.merge(BENCH, base, branch, delete=True))
        merges.append(ms)
        if d["delta_count"] != 100 or merged != 100:
            raise RuntimeError(f"merge round {round_}: diff {d['delta_count']}, merged {merged}")
    out.append(row("diff", "100 staged chunks", summarize(diffs)))
    out.append(row("merge", "100 staged chunks", summarize(merges), "includes delete_branch_after_merge"))

    # ─── Mode C (SQ8 + mmap) vs Mode A at the same size ──────────
    n = 2000
    mode_a = ctx.pipe("mmu_bench", f"a{n}")
    mode_c = ctx.pipe("mmu_bulk", f"bench{n}")
    fill(c, BENCH, mode_a, n)
    fill(c, BULK, mode_c, n)  # mmu_bulk migrates to Mode C at 256 chunks
    queries = query_set(n)
    for label, app, pipe in (("Mode A (f32 RAM)", BENCH, mode_a), ("Mode C (SQ8 + mmap)", BULK, mode_c)):
        run = lambda i, ap=app, p=pipe: c.search(ap, p, queries[i % len(queries)], top_k=5)  # noqa: E731
        out.append(row("mode_search", f"{label} n={n}", summarize(sample(run, it, max(warm, len(queries))))))

    # ─── Message Bus round trip ──────────────────────────────────
    c.drain(WORKER_A)

    def ping(i):
        c.send(COORDINATOR, WORKER_A, {"run": ctx.run_id, "ping": i})
        if c.listen(WORKER_A) is None:
            raise RuntimeError("ping lost")

    out.append(row("msgbus_rtt", "send + listen", summarize(sample(ping, it, warm)), "no broker process"))
    return out
