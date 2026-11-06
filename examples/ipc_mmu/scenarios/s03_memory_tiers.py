"""s03 - Memory tiers: fact / skill / directive tags and filter_category."""

import corpus
from harness import scenario

from .common import RESEARCHER


@scenario("s03_memory_tiers", "Memory tiers: category tags and filter_category")
def run(ctx):
    c = ctx.client
    pipe = ctx.pipe("mmu_facts", "s03")
    for category in ("fact", "skill", "directive", "general"):
        c.share_docs(RESEARCHER, pipe, [d.text for d in corpus.docs_in(category)], category=category)

    expected = {d.text: d.category for d in corpus.CORE_DOCS}
    query = "what should an operator do about the certificate outage"

    mixed = c.search(RESEARCHER, pipe, query, top_k=10)
    ctx.expect(len({r["category"] for r in mixed}) >= 2, "an unfiltered search should span several tiers")
    for r in mixed:
        ctx.expect(r["category"] == expected[r["text"]], f"chunk lost its tag: {r['text'][:50]} is {r['category']}")

    for category in ("fact", "skill", "directive"):
        results = c.search(RESEARCHER, pipe, query, top_k=10, filter_category=category)
        ctx.expect(results, f"filter_category={category} returned nothing")
        wrong = [r["category"] for r in results if r["category"] != category]
        ctx.expect(not wrong, f"filter_category={category} leaked other tiers: {wrong}")
        ctx.metric(f"{category}_hits", len(results))

    skill = c.search(RESEARCHER, pipe, "how do I replace an expired certificate", top_k=1, filter_category="skill")
    ctx.expect(skill and skill[0]["text"] == corpus.DOC_BY_ID["k01"].text, "skill tier should answer the how-to query")
