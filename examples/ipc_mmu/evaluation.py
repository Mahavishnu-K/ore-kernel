"""
evaluation.py - Builds the labelled evaluation index and runs the query set.

Shared by the retrieval and calibration scenarios and by the benchmarks, so all
of them measure the same index the same way:
  - documents in corpus.EVAL_DOCS are stored whole, one chunk each, in their tier
  - corpus.LONG_DOCS are chunked with sentence_window (the recommended setting)
  - every query runs dense-only and hybrid, top 10
"""

import time
from dataclasses import dataclass, field

import corpus
import metrics

TOP_K = 10
LONG_DOC_STRATEGY = {"chunk_strategy": "sentence_window", "chunk_size": 60}


@dataclass
class EvalRun:
    pipe: str
    index_s: float = 0.0
    query_s: dict = field(default_factory=dict)  # mode -> seconds for the whole query set
    results: dict = field(default_factory=dict)  # mode -> {query id -> results}


def build_index(client, app: str, pipe: str) -> float:
    """Stores the evaluation corpus in `pipe`. Returns seconds taken."""
    start = time.perf_counter()
    for category in ("fact", "skill", "directive", "general"):
        docs = [d.text for d in corpus.EVAL_DOCS if d.category == category]
        if docs:
            client.share_docs(app, pipe, docs, category=category)
    for doc in corpus.LONG_DOCS:
        client.share(app, pipe, doc.text, category=doc.category, **LONG_DOC_STRATEGY)
    return time.perf_counter() - start


def run_queries(client, app: str, pipe: str, hybrid: bool, queries: list = None) -> tuple:
    """Returns ({query id: results}, seconds)."""
    out = {}
    start = time.perf_counter()
    for q in queries or corpus.EVAL_QUERIES:
        out[q.id] = client.search(app, pipe, q.text, top_k=TOP_K, enable_hybrid=hybrid)
    return out, time.perf_counter() - start


def evaluate(client, app: str, pipe: str) -> EvalRun:
    run = EvalRun(pipe)
    run.index_s = build_index(client, app, pipe)
    # Dense first: it embeds every query once, so the hybrid pass reads the
    # embedding cache and the two timings are comparable.
    for mode, hybrid in (("dense", False), ("hybrid", True)):
        run.results[mode], run.query_s[mode] = run_queries(client, app, pipe, hybrid)
    return run


def cached_run(ctx, app: str) -> EvalRun:
    """One evaluation per suite run, shared by every scenario that needs it."""
    if "eval_run" not in ctx.shared:
        ctx.shared["eval_run"] = evaluate(ctx.client, app, ctx.pipe("mmu_facts", "eval"))
    return ctx.shared["eval_run"]


def scores(run: EvalRun, mode: str) -> list:
    """QueryScore for every answerable query in one mode."""
    return [
        metrics.score_query(q, run.results[mode][q.id])
        for q in corpus.EVAL_QUERIES
        if q.answerable
    ]


def top_score(results: list) -> float:
    return results[0]["score"] if results else 0.0


def confidence_signals(run: EvalRun) -> dict:
    """Per query, the confidence each gate design would see:
    dense         top cosine similarity (dense-only search)
    hybrid_fused  top normalized RRF score (hybrid search)
    kernel_gate   max(hybrid_fused, dense), what CRAG compares today
    """
    signals = {"dense": {}, "hybrid_fused": {}, "kernel_gate": {}}
    for q in corpus.EVAL_QUERIES:
        dense = top_score(run.results["dense"][q.id])
        fused = top_score(run.results["hybrid"][q.id])
        signals["dense"][q.id] = dense
        signals["hybrid_fused"][q.id] = fused
        signals["kernel_gate"][q.id] = max(dense, fused)
    return signals
