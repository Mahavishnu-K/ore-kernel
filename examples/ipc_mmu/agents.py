"""
agents.py - Agent roles for the Cognitive MMU multi-agent workflow.

Each agent is a Python object bound to one Ore manifest (app_id). They talk to
each other only through the kernel:
  - Message Bus (/ipc/send, /ipc/listen) for tasks, hand-offs, and verdicts
  - Semantic Bus (/ipc/share, /ipc/search) for shared memory
  - CoW branches (/ipc/pipe/*) so worker writes are reviewed before they land

Messages are JSON envelopes: {"run", "kind", "seq", ...}. `run` keeps
leftovers from earlier runs out of this one; `seq` lets the receiver check
the bus delivered each sender's messages in order.
"""

import json
import threading
import time
from collections import defaultdict

import corpus


class Agent:
    def __init__(self, client, app_id: str, run_id: str):
        self.client = client
        self.app_id = app_id
        self.run_id = run_id
        self.inbox: list = []
        self._seq = defaultdict(int)
        self._last_seen = {}
        self.order_violations: list = []

    def __repr__(self):
        return f"<{type(self).__name__} {self.app_id}>"

    # ─── Message Bus ─────────────────────────────────────────────

    def send(self, to_app: str, kind: str, **data) -> None:
        self._seq[to_app] += 1
        envelope = {"run": self.run_id, "kind": kind, "seq": self._seq[to_app], **data}
        reply = self.client.send(self.app_id, to_app, envelope)
        if not reply.startswith("SUCCESS"):
            raise RuntimeError(f"{self.app_id} -> {to_app}: {reply}")

    def _accept(self, msg: dict):
        try:
            body = json.loads(msg["payload"])
        except (KeyError, ValueError):
            return None
        if body.get("run") != self.run_id:
            return None  # left over from another run
        sender = msg["from_app"]
        last = self._last_seen.get(sender, 0)
        if body.get("seq", 0) <= last:
            self.order_violations.append((sender, last, body.get("seq")))
        self._last_seen[sender] = body.get("seq", 0)
        body["from_app"] = sender
        return body

    def recv(self, kind: str = None, timeout: float = 120.0, poll: float = 0.05) -> dict:
        """Next message of `kind` for this run. Other kinds stay in the inbox."""
        deadline = time.monotonic() + timeout
        while True:
            for i, body in enumerate(self.inbox):
                if kind is None or body["kind"] == kind:
                    return self.inbox.pop(i)
            msg = self.client.listen(self.app_id)
            if msg is not None:
                body = self._accept(msg)
                if body is not None:
                    self.inbox.append(body)
                continue
            if time.monotonic() > deadline:
                raise TimeoutError(f"{self.app_id} waited {timeout:.0f}s for a '{kind}' message")
            time.sleep(poll)

    def reset(self) -> int:
        """Drops everything queued for this agent (stale runs, failed tests)."""
        self.inbox.clear()
        return len(self.client.drain(self.app_id))

    # ─── Semantic Bus ────────────────────────────────────────────

    def search(self, pipe: str, query: str, **kw) -> list:
        return self.client.search(self.app_id, pipe, query, **kw)

    def share_docs(self, pipe: str, docs: list, category: str = None) -> int:
        return self.client.share_docs(self.app_id, pipe, docs, category=category)


class Coordinator(Agent):
    """Hands out tasks and collects results. Owns no memory."""


class Researcher(Agent):
    """Gathers facts into a shared, read-mostly pipe."""

    def serve_once(self, timeout: float = 120.0) -> int:
        task = self.recv("task", timeout)
        written = 0
        for category in ("fact", "skill", "directive"):
            docs = [d.text for d in corpus.docs_in(category)]
            written += self.share_docs(task["facts_pipe"], docs, category=category)
        self.send(task["from_app"], "done", stage="research", written=written)
        return written


class Worker(Agent):
    """Reads shared facts, writes findings into its own CoW branch, asks for review."""

    def __init__(self, *args, poison: bool = False, **kw):
        super().__init__(*args, **kw)
        self.poison = poison

    def serve_once(self, timeout: float = 120.0) -> list:
        task = self.recv("task", timeout)
        findings = []
        for i, query in enumerate(task["queries"]):
            hits = self.search(task["facts_pipe"], query, top_k=1, filter_category="fact")
            if hits:
                findings.append(f"[{self.run_id}] {self.app_id} finding {i}: {hits[0]['text']}")
        for text in findings:
            self.client.share(self.app_id, task["branch"], text, category="fact", chunk_strategy="exact_match")
        written = len(findings)
        if self.poison:
            # A worker has no business writing directives. The reviewer must catch it.
            self.client.share(
                self.app_id, task["branch"], corpus.POISON_TEXT, category="directive", chunk_strategy="exact_match"
            )
            written += 1
        self.send(task["reviewer"], "review_request", branch=task["branch"], base=task["base"], written=written)
        self.send(task["from_app"], "done", stage="work", branch=task["branch"], findings=findings)
        return findings


class Reviewer(Agent):
    """Owns the shared knowledge base. Opens branches, reviews diffs, merges or rejects."""

    def open_branch(self, base: str, branch: str) -> None:
        self.client.branch(self.app_id, base, branch)

    @staticmethod
    def policy_violations(staged: list) -> list:
        reasons = []
        for c in staged:
            if c["category"] == "directive":
                reasons.append(f"chunk {c['id']} writes the directive tier")
            if "ignore previous instructions" in c["text"].lower():
                reasons.append(f"chunk {c['id']} contains a prompt injection")
        return reasons

    def serve(self, expected_requests: int, coordinator: str, timeout: float = 180.0) -> list:
        verdicts = []
        for _ in range(expected_requests):
            req = self.recv("review_request", timeout)
            diff = self.client.diff(self.app_id, req["base"], req["branch"])
            verdict = {"branch": req["branch"], "delta_count": diff["delta_count"], "claimed": req["written"]}
            if diff["delta_count"] != req["written"]:
                verdict.update(verdict="rejected", reason="delta count does not match the worker's claim")
            elif reasons := self.policy_violations(diff["staged_chunks"]):
                verdict.update(verdict="rejected", reason="; ".join(reasons))
            else:
                merged = self.client.merge(self.app_id, req["base"], req["branch"], delete=True)
                verdict.update(verdict="merged", merged=merged)
            self.send(coordinator, "review", **verdict)
            verdicts.append(verdict)
        return verdicts


class Analyst(Agent):
    """Consumes reviewed knowledge: search, or a full RAG turn through POST /infer."""

    def ask(self, prompt: str) -> str:
        r = self.client.infer(self.app_id, prompt)
        if r.status_code != 200:
            raise RuntimeError(f"/infer -> {r.status_code}: {r.text[:300]}")
        return r.text


def run_threads(targets: list, timeout: float = 300.0) -> list:
    """Runs (fn, args) pairs concurrently. Returns results in order; re-raises the first error."""
    results = [None] * len(targets)
    errors = [None] * len(targets)

    def wrap(i, fn, args):
        try:
            results[i] = fn(*args)
        except BaseException as e:  # surfaced to the caller below
            errors[i] = e

    threads = [threading.Thread(target=wrap, args=(i, fn, args), daemon=True) for i, (fn, args) in enumerate(targets)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout)
    for e in errors:
        if e is not None:
            raise e
    if any(t.is_alive() for t in threads):
        raise TimeoutError("agent thread did not finish in time")
    return results
