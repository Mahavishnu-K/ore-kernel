"""s13 - Retrieval quality on the labelled corpus: Recall@k, MRR, nDCG@10 by query kind.

132 queries over 79 whole documents and 10 chunked long documents, run dense-only
and hybrid. Hard assertions cover exact lookups, where hybrid search must be
reliable; everything else is reported for tracking. CJK recall is a known gap
(English embedder, whitespace tokenizer) and ends as XFAIL until it improves.
"""

import corpus
import evaluation
import metrics
from harness import scenario

from .common import RESEARCHER


def row(rows: list, label: str):
    return next(r for r in rows if r.label == label)


@scenario("s13_retrieval_eval", "Retrieval quality: Recall@k, MRR, nDCG@10 by query kind")
def run(ctx):
    run = evaluation.cached_run(ctx, RESEARCHER)
    dense = metrics.by_kind(evaluation.scores(run, "dense"))
    hybrid = metrics.by_kind(evaluation.scores(run, "hybrid"))

    queries = {q.id: q for q in corpus.EVAL_QUERIES}
    n = len(corpus.EVAL_QUERIES)
    ctx.section(
        "Retrieval quality",
        f"{len(corpus.EVAL_DOCS)} whole documents + {len(corpus.LONG_DOCS)} chunked long documents, "
        f"{sum(q.answerable for q in corpus.EVAL_QUERIES)} answerable queries "
        f"(+{sum(not q.answerable for q in corpus.EVAL_QUERIES)} unanswerable, used in calibration). "
        f"Index built in {run.index_s:.1f}s; dense pass {run.query_s['dense'] / n * 1000:.0f} ms/query "
        f"(includes embedding each query), hybrid pass {run.query_s['hybrid'] / n * 1000:.0f} ms/query "
        "(embedding cached).\n\n" + metrics.retrieval_table({"dense": dense, "hybrid": hybrid}),
    )
    ctx.section("Hardest queries (hybrid)", metrics.misses_table(evaluation.scores(run, "hybrid"), queries))

    for mode, rows in (("dense", dense), ("hybrid", hybrid)):
        overall = row(rows, "all answerable")
        ctx.metric(f"{mode}_mrr", round(overall.mrr, 3))
        ctx.metric(f"{mode}_recall@5", round(overall.recall[5], 3))
        ctx.metric(f"{mode}_ndcg@10", round(overall.ndcg10, 3))

    h_exact, d_exact = row(hybrid, "exact"), row(dense, "exact")
    h_tech, d_tech = row(hybrid, "technical"), row(dense, "technical")
    ctx.expect(h_exact.hit[5] >= 0.875, f"hybrid must find exact IDs: hit@5 = {h_exact.hit[5]:.2f}")
    ctx.expect(h_exact.hit[5] >= d_exact.hit[5], "hybrid lost exact-ID recall to dense-only")
    ctx.expect(h_tech.hit[5] >= 0.75, f"hybrid must find technical identifiers: hit@5 = {h_tech.hit[5]:.2f}")
    ctx.expect(h_tech.mrr >= d_tech.mrr - 0.05, f"hybrid hurt technical MRR: {h_tech.mrr:.3f} vs {d_tech.mrr:.3f}")
    ctx.expect(row(hybrid, "long").hit[5] >= 0.75, "sentence-window long-document answers should be in the top 5")

    cjk = row(hybrid, "cjk")
    ctx.metric("cjk_hit@5", round(cjk.hit[5], 3))
    ctx.known_gap(cjk.hit[5] >= 0.6, f"CJK/non-Latin hit@5 is {cjk.hit[5]:.2f} (English embedder, whitespace tokenizer)")
