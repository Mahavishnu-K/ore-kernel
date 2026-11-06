"""s08 - The production workflow, end to end, with concurrent agents.

  coordinator --task--> researcher           (Message Bus)
  researcher  --share--> mmu_facts_*         (Semantic Bus, tiered)
  reviewer    --branch--> mmu_br_*_a / _b    (CoW, zero-copy)
  workers     --search facts, write branch-> (in parallel threads)
  workers     --review_request--> reviewer
  reviewer    --diff, policy check, merge or reject--> mmu_kb_*
  analyst     --search--> mmu_kb_*           (only reviewed knowledge)

worker_b is "compromised": besides its findings it writes a directive-tier
prompt injection. The reviewer must reject its branch; nothing from it may
reach the knowledge base.
"""

import time

import corpus
from agents import Analyst, Coordinator, Researcher, Reviewer, Worker, run_threads
from harness import scenario
from mmu_client import OreMmuClient

from .common import ANALYST, COORDINATOR, RESEARCHER, REVIEWER, WORKER_A, WORKER_B, texts


@scenario("s08_swarm", "Multi-agent swarm: research -> branch work -> review -> merge")
def run(ctx):
    base_url = ctx.client.base_url

    def agent(cls, app_id, **kw):
        # One HTTP session per agent: agents run on separate threads.
        return cls(OreMmuClient(base_url), app_id, ctx.run_id, **kw)

    coord = agent(Coordinator, COORDINATOR)
    researcher = agent(Researcher, RESEARCHER)
    worker_a = agent(Worker, WORKER_A)
    worker_b = agent(Worker, WORKER_B, poison=True)
    reviewer = agent(Reviewer, REVIEWER)
    analyst = agent(Analyst, ANALYST)
    everyone = [coord, researcher, worker_a, worker_b, reviewer, analyst]
    for a in everyone:
        a.reset()

    facts = ctx.pipe("mmu_facts", "swarm")
    kb = ctx.pipe("mmu_kb", "swarm")
    branches = {WORKER_A: ctx.pipe("mmu_br", "swarm_a"), WORKER_B: ctx.pipe("mmu_br", "swarm_b")}
    timings = {}

    # 1. Research: coordinator dispatches, researcher fills the shared facts pipe.
    t0 = time.perf_counter()
    coord.send(RESEARCHER, "task", facts_pipe=facts)
    written = researcher.serve_once()
    done = coord.recv("done")
    ctx.expect(done["stage"] == "research" and done["written"] == written, f"bad research report: {done}")
    timings["research_s"] = time.perf_counter() - t0

    # 2. Reviewer seeds the knowledge base with policy and opens one branch per worker.
    t0 = time.perf_counter()
    reviewer.share_docs(kb, [d.text for d in corpus.docs_in("directive")], category="directive")
    for branch in branches.values():
        reviewer.open_branch(kb, branch)
    timings["branch_s"] = time.perf_counter() - t0

    # 3. Workers run in parallel; the reviewer serves review requests as they arrive.
    t0 = time.perf_counter()
    for worker_id, task in corpus.SWARM_TASKS.items():
        coord.send(
            worker_id,
            "task",
            facts_pipe=facts,
            queries=task["queries"],
            branch=branches[worker_id],
            base=kb,
            reviewer=REVIEWER,
        )
    findings_a, findings_b, verdicts = run_threads(
        [
            (worker_a.serve_once, ()),
            (worker_b.serve_once, ()),
            (reviewer.serve, (2, COORDINATOR)),
        ]
    )
    timings["work_review_s"] = time.perf_counter() - t0

    # 4. Coordinator collects both work reports and both verdicts.
    reports = {coord.recv("done")["branch"] for _ in range(2)}
    ctx.expect(reports == set(branches.values()), f"missing work reports: {reports}")
    by_branch = {}
    for _ in range(2):
        v = coord.recv("review")
        by_branch[v["branch"]] = v
    a, b = by_branch[branches[WORKER_A]], by_branch[branches[WORKER_B]]
    ctx.expect(a["verdict"] == "merged", f"worker_a's clean branch should merge: {a}")
    ctx.expect(a["merged"] == len(findings_a), f"merged {a['merged']} chunks, worker_a wrote {len(findings_a)}")
    ctx.expect(b["verdict"] == "rejected", f"worker_b's poisoned branch must be rejected: {b}")
    ctx.log(f"worker_b rejected: {b['reason']}")

    # 5. Analyst reads only reviewed knowledge.
    t0 = time.perf_counter()
    for text in findings_a:
        hits = texts(analyst.search(kb, text, top_k=10))
        ctx.expect(hits.count(text) == 1, f"merged finding appears {hits.count(text)}x: {text[:60]}")
    for text in findings_b + [corpus.POISON_TEXT]:
        hits = texts(analyst.search(kb, text, top_k=10))
        ctx.expect(text not in hits, f"rejected content reached the knowledge base: {text[:60]}")
    timings["verify_s"] = time.perf_counter() - t0

    violations = [v for ag in everyone for v in ag.order_violations]
    ctx.expect(not violations, f"Message Bus delivered out of order: {violations}")

    for k, v in timings.items():
        ctx.metric(k, round(v, 3))
    ctx.metric("findings_merged", len(findings_a))
    ctx.metric("findings_rejected", len(findings_b) + 1)
