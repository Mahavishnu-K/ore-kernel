"""s02 - Hybrid retrieval: BM25 + dense fused with RRF vs dense only.

Exact identifiers (ERR_7731, pg-orders-07) carry almost no meaning for an
embedding model; BM25 matches them as tokens. Hybrid search must find them at
least as often as dense search does, without losing semantic queries.
"""

import corpus
from harness import scenario

from .common import RESEARCHER, rank_of


def recall_at(ctx, pipe: str, queries: list, hybrid: bool, k: int) -> float:
    hits = 0
    for q in queries:
        results = ctx.client.search(RESEARCHER, pipe, q.text, top_k=k, enable_hybrid=hybrid)
        if rank_of(results, corpus.DOC_BY_ID[q.relevant[0]].text) is not None:
            hits += 1
    return hits / len(queries)


@scenario("s02_hybrid_bm25", "Hybrid (BM25 + dense, RRF) vs dense-only recall@3")
def run(ctx):
    pipe = ctx.pipe("mmu_facts", "s02")
    ctx.client.share_docs(RESEARCHER, pipe, [d.text for d in corpus.CORE_DOCS])

    exact = [q for q in corpus.CORE_QUERIES if q.kind == "exact"]
    semantic = [q for q in corpus.CORE_QUERIES if q.kind == "semantic"]

    exact_hybrid = recall_at(ctx, pipe, exact, True, 3)
    exact_dense = recall_at(ctx, pipe, exact, False, 3)
    sem_hybrid = recall_at(ctx, pipe, semantic, True, 3)
    sem_dense = recall_at(ctx, pipe, semantic, False, 3)

    ctx.metric("exact_recall@3_hybrid", exact_hybrid)
    ctx.metric("exact_recall@3_dense", exact_dense)
    ctx.metric("semantic_recall@3_hybrid", sem_hybrid)
    ctx.metric("semantic_recall@3_dense", sem_dense)
    ctx.log(f"exact IDs  hybrid={exact_hybrid:.2f} dense={exact_dense:.2f}")
    ctx.log(f"semantic   hybrid={sem_hybrid:.2f} dense={sem_dense:.2f}")

    ctx.expect(exact_hybrid >= exact_dense, f"hybrid lost exact-ID recall: {exact_hybrid:.2f} < {exact_dense:.2f}")
    ctx.expect(exact_hybrid >= 0.875, f"hybrid should find nearly every exact ID in the top 3, got {exact_hybrid:.2f}")
    ctx.expect(sem_hybrid >= sem_dense - 0.125, f"hybrid hurt semantic recall: {sem_hybrid:.2f} vs {sem_dense:.2f}")
