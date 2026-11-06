"""s05 - Copy-on-write branches: isolation, diff, merge, idempotence, delete."""

import corpus
from harness import scenario
from mmu_client import KernelError

from .common import REVIEWER, rank_of, texts


@scenario("s05_cow_branches", "CoW branches: branch -> write -> diff -> merge (no duplicates)")
def run(ctx):
    c = ctx.client
    base = ctx.pipe("mmu_kb", "s05")
    branch = ctx.pipe("mmu_br", "s05")
    missing = ctx.pipe("mmu_kb", "s05_missing")

    r = c.branch_raw(REVIEWER, missing, ctx.pipe("mmu_br", "s05_orphan"))
    ctx.expect(r.status_code == 400, f"branching a pipe that doesn't exist should be 400, got {r.status_code}")

    seed = [d.text for d in corpus.docs_in("directive")]
    c.share_docs(REVIEWER, base, seed, category="directive")
    c.branch(REVIEWER, base, branch)
    r = c.branch_raw(REVIEWER, base, branch)
    ctx.expect(r.status_code == 400, f"re-creating an existing branch should be 400, got {r.status_code}")

    staged = [f"[{ctx.run_id}] staged finding {i}: shard pg-orders-0{i} is healthy after the resize." for i in range(3)]
    for text in staged:
        c.share(REVIEWER, branch, text, category="fact", chunk_strategy="exact_match")

    # Isolation: the base doesn't see staged chunks; the branch sees base + delta.
    ctx.expect(rank_of(c.search(REVIEWER, base, staged[0], top_k=10), staged[0]) is None, "staged chunk leaked into the base")
    in_branch = texts(c.search(REVIEWER, branch, staged[0], top_k=10))
    ctx.expect(staged[0] in in_branch, "branch can't see its own write")
    ctx.expect(rank_of(c.search(REVIEWER, branch, seed[0], top_k=3), seed[0]) is not None, "branch can't see base chunks")

    d = c.diff(REVIEWER, base, branch)
    ctx.expect(d["delta_count"] == 3, f"diff should stage 3 chunks, got {d['delta_count']}")
    ctx.expect(sorted(s["text"] for s in d["staged_chunks"]) == sorted(staged), "diff returned the wrong chunks")

    r = c.diff_raw(REVIEWER, ctx.pipe("mmu_kb", "s05_other"), branch)
    ctx.expect(r.status_code == 400, f"diff against the wrong base should be 400, got {r.status_code}")

    ctx.expect(c.merge(REVIEWER, base, branch) == 3, "first merge should commit 3 chunks")
    ctx.expect(c.diff(REVIEWER, base, branch)["delta_count"] == 0, "branch must be clean after a merge")
    ctx.expect(c.merge(REVIEWER, base, branch) == 0, "merging twice must not re-apply the delta")

    for text in staged:
        for pipe in (base, branch):
            hits = texts(c.search(REVIEWER, pipe, text, top_k=10))
            ctx.expect(hits.count(text) == 1, f"'{text[:40]}…' appears {hits.count(text)}x in {pipe}")

    late = f"[{ctx.run_id}] late finding: replication lag stayed under one second."
    c.share(REVIEWER, branch, late, category="fact", chunk_strategy="exact_match")
    ctx.expect(c.merge(REVIEWER, base, branch, delete=True) == 1, "merge-and-delete should commit 1 chunk")
    try:
        c.diff(REVIEWER, base, branch)
        raise AssertionError("branch should be gone after delete_branch_after_merge")
    except KernelError as e:
        ctx.expect(e.status == 400, f"diff of a deleted branch should be 400, got {e.status}")
    ctx.expect(rank_of(c.search(REVIEWER, base, late, top_k=5), late) == 1, "late chunk missing from the base")
