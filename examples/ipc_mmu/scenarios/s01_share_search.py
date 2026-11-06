"""s01 - Write/read round trip: the contract every other scenario depends on."""

import corpus
from harness import scenario

from .common import RESEARCHER, rank_of

FIELDS = {"text", "context_payload", "score", "source_app", "timestamp", "category", "is_crag_fallback"}


@scenario("s01_share_search", "Share -> search round trip, scores, top_k, determinism")
def run(ctx):
    c = ctx.client
    pipe = ctx.pipe("mmu_facts", "s01")
    docs = [d.text for d in corpus.CORE_DOCS]

    c.share_docs(RESEARCHER, pipe, docs)

    empty = c.share_raw(RESEARCHER, pipe, "   ")
    ctx.expect(empty.startswith("SUCCESS"), f"empty share should be a no-op SUCCESS, got: {empty}")

    target = corpus.DOC_BY_ID["f02"].text
    results = c.search(RESEARCHER, pipe, target, top_k=5)
    ctx.expect(0 < len(results) <= 5, f"expected 1..5 results, got {len(results)}")
    ctx.expect(FIELDS <= set(results[0]), f"result is missing fields: {FIELDS - set(results[0])}")
    ctx.expect(rank_of(results, target) == 1, "searching a stored text verbatim must rank it first")

    scores = [r["score"] for r in results]
    ctx.expect(all(0.0 <= s <= 1.0 for s in scores), f"scores must be normalized to [0, 1]: {scores}")
    ctx.expect(scores == sorted(scores, reverse=True), f"results must be sorted by score: {scores}")
    ctx.expect(all(r["source_app"] == RESEARCHER for r in results), "source_app must be the writing agent")
    ctx.expect(not any(r["is_crag_fallback"] for r in results), "no CRAG fallback was configured")

    one = c.search(RESEARCHER, pipe, target, top_k=1)
    ctx.expect(len(one) == 1, f"top_k=1 returned {len(one)} results")

    query = "payment outage caused by an expired certificate"
    first = [(r["text"], r["score"]) for r in c.search(RESEARCHER, pipe, query, top_k=8)]
    for _ in range(3):
        again = [(r["text"], r["score"]) for r in c.search(RESEARCHER, pipe, query, top_k=8)]
        ctx.expect(again == first, "identical searches must return identical order and scores")

    ctx.metric("docs", len(docs))
