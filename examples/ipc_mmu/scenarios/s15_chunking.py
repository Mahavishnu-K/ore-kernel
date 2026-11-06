"""s15 - Chunking strategies on long postmortems and runbooks.

Stores the ten long documents once per strategy and asks the 32 long-document
questions. For each strategy, the report shows how many chunks were made, how
big the probe (embedded) and payload (returned) texts are, and how often the
answer lands in the probe versus only in the payload. Also checks the CJK
sentence split (a known gap: Chinese punctuation has no trailing space).
"""

import statistics

import corpus
import metrics
from harness import scenario

from .common import RESEARCHER

STRATEGIES = {
    "sliding_window": {"chunk_size": 80, "chunk_overlap": 16},
    "sentence_aware": {"chunk_size": 80, "chunk_overlap": 16},
    "sentence_window": {"chunk_size": 60},
    "paragraph": {"chunk_size": 120},
}
ALL_CHUNKS = 1000  # larger than any pipe here, so a search returns every chunk


def contains(text: str, phrases: tuple) -> bool:
    return any(p.lower() in text.lower() for p in phrases)


@scenario("s15_chunking", "Chunking strategies on long documents (+ CJK sentence split)")
def run(ctx):
    c = ctx.client
    queries = corpus.queries_of("long")
    rows, by_strategy = [], {}

    for strategy, params in STRATEGIES.items():
        pipe = ctx.pipe("mmu_facts", f"chunk_{strategy}")
        for doc in corpus.LONG_DOCS:
            c.share(RESEARCHER, pipe, doc.text, category=doc.category, chunk_strategy=strategy, **params)

        chunks = c.search(RESEARCHER, pipe, "incident runbook", top_k=ALL_CHUNKS, enable_hybrid=False)
        probe_words = [len(r["text"].split()) for r in chunks]
        payload_words = [len(r["context_payload"].split()) for r in chunks]

        probe_hit1 = payload_hit1 = payload_hit5 = 0
        scores = []
        for q in queries:
            results = c.search(RESEARCHER, pipe, q.text, top_k=5)
            scores.append(metrics.score_query(q, results))
            if results and contains(results[0]["text"], q.answers):
                probe_hit1 += 1
            if results and contains(results[0]["context_payload"], q.answers):
                payload_hit1 += 1
            if any(contains(r["context_payload"], q.answers) for r in results):
                payload_hit5 += 1

        n = len(queries)
        summary = {
            "chunks": len(chunks),
            "probe_words": statistics.fmean(probe_words) if probe_words else 0,
            "payload_words": statistics.fmean(payload_words) if payload_words else 0,
            "probe_hit1": probe_hit1 / n,
            "payload_hit1": payload_hit1 / n,
            "payload_hit5": payload_hit5 / n,
            "mrr": statistics.fmean(s.rr for s in scores),
        }
        by_strategy[strategy] = summary
        rows.append(
            f"| {strategy} | {params} | {summary['chunks']} | {summary['probe_words']:.1f} | "
            f"{summary['payload_words']:.1f} | {summary['probe_hit1']:.2f} | {summary['payload_hit1']:.2f} | "
            f"{summary['payload_hit5']:.2f} | {summary['mrr']:.3f} |"
        )
        ctx.metric(f"{strategy}_payload_hit@5", round(summary["payload_hit5"], 3))

    # CJK: six sentences ending in "。" with no spaces.
    cjk_pipe = ctx.pipe("mmu_facts", "chunk_cjk")
    c.share(RESEARCHER, cjk_pipe, corpus.cjk.CJK_PARAGRAPH, chunk_strategy="sentence_window", chunk_size=60)
    cjk_chunks = c.search(RESEARCHER, cjk_pipe, corpus.cjk.CJK_SENTENCES[0], top_k=ALL_CHUNKS, enable_hybrid=False)
    expected = len(corpus.cjk.CJK_SENTENCES)
    ctx.metric("cjk_sentence_window_chunks", len(cjk_chunks))

    ctx.section(
        "Chunking strategies (long documents)",
        "\n".join(
            [
                f"{len(corpus.LONG_DOCS)} documents, {len(queries)} questions, top 5. Probe = the embedded text that "
                "was matched; payload = the context returned to the agent.",
                "",
                "| Strategy | Params | Chunks | Probe words | Payload words | Answer in probe @1 | "
                "Answer in payload @1 | Answer in payload @5 | MRR |",
                "|---|---|---:|---:|---:|---:|---:|---:|---:|",
                *rows,
                "",
                f"CJK paragraph with {expected} sentences, sentence_window: {len(cjk_chunks)} chunk(s).",
            ]
        ),
    )

    sw = by_strategy["sentence_window"]
    ctx.expect(sw["payload_hit5"] >= 0.75, f"sentence_window found {sw['payload_hit5']:.0%} of answers in the top 5")
    ctx.expect(sw["payload_words"] > sw["probe_words"], "sentence_window payloads should be wider than their probes")
    for strategy, s in by_strategy.items():
        ctx.expect(s["payload_hit5"] >= 0.5, f"{strategy} found only {s['payload_hit5']:.0%} of answers in the top 5")
    ctx.known_gap(
        len(cjk_chunks) == expected,
        f"sentence_window made {len(cjk_chunks)} chunk(s) from {expected} Chinese sentences "
        "(splits only on . ! ? followed by a space)",
    )
