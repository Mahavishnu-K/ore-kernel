"""s06 - Kernel-enforced isolation.

In a typical multi-agent stack, every agent holds the same vector-DB
credentials and isolation is a convention. Here each agent's manifest is the
access-control list, and the kernel checks it on every call.
"""

from harness import scenario

from .common import COORDINATOR, GHOST, OUTSIDER, REVIEWER, WORKER_A, rank_of


@scenario("s06_permissions", "Manifest isolation: 401 / 403 / denied writes / denied messages")
def run(ctx):
    c = ctx.client
    base = ctx.pipe("mmu_kb", "s06")
    branch = ctx.pipe("mmu_br", "s06_worker_a")
    probe = f"[{ctx.run_id}] reviewed fact: the Atlas capacity review moved to Wednesday."
    c.share(REVIEWER, base, probe, category="fact", chunk_strategy="exact_match")
    c.branch(REVIEWER, base, branch)

    # Unregistered agents.
    r = c.search_raw(GHOST, base, "anything")
    ctx.expect(r.status_code == 401, f"ghost search should be 401, got {r.status_code}")
    body = c.share_raw(GHOST, base, "ghost write")
    ctx.expect("Unregistered" in body, f"ghost share should be refused, got: {body}")
    ctx.expect(c.listen(GHOST) is None, "ghost listen must return nothing")

    # Agents without the pipe in their manifest.
    r = c.search_raw(OUTSIDER, base, "Atlas")
    ctx.expect(r.status_code == 403, f"outsider search should be 403, got {r.status_code}")
    body = c.share_raw(OUTSIDER, base, "outsider write")
    ctx.expect("Permission Denied" in body, f"outsider share should be refused, got: {body}")

    # A worker may write its branch, but never the reviewed base.
    poisoned = f"[{ctx.run_id}] unreviewed claim written straight to the base"
    body = c.share_raw(WORKER_A, base, poisoned, chunk_strategy="exact_match")
    ctx.expect("Permission Denied" in body, f"worker write to the base should be refused, got: {body}")
    c.share(WORKER_A, branch, f"[{ctx.run_id}] worker draft", chunk_strategy="exact_match")

    r = c.merge_raw(WORKER_A, base, branch)
    ctx.expect(r.status_code == 403, f"worker merge should be 403, got {r.status_code}")
    r = c.branch_raw(WORKER_A, base, ctx.pipe("mmu_br", "s06_self_made"))
    ctx.expect(r.status_code == 403, f"worker can't fork the base it can't read, got {r.status_code}")
    r = c.diff_raw(WORKER_A, base, branch)
    ctx.expect(r.status_code == 200, f"worker may diff its own branch, got {r.status_code}")

    ctx.expect(
        rank_of(c.search(REVIEWER, base, poisoned, top_k=5), poisoned) is None,
        "a denied write reached the base pipe",
    )

    # Message Bus: targets come from allowed_agent_targets.
    reply = c.send(WORKER_A, OUTSIDER, {"run": ctx.run_id, "kind": "spam"})
    ctx.expect("not in allowed_agent_targets" in reply, f"worker -> outsider should be refused, got: {reply}")
    reply = c.send(OUTSIDER, COORDINATOR, {"run": ctx.run_id, "kind": "spam"})
    ctx.expect("not in allowed_agent_targets" in reply, f"outsider -> coordinator should be refused, got: {reply}")
    reply = c.send(GHOST, COORDINATOR, "hello")
    ctx.expect("Unregistered sender" in reply, f"ghost sender should be refused, got: {reply}")
