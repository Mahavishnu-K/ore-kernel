"""
mmu_client.py - Typed client for every Cognitive MMU route.

Extends the shared OreClient (token discovery, ORE_URL) with wrappers for
share/search, CoW branches, RAPTOR, the Message Bus, and POST /infer.

Error handling follows the kernel's contract:
  - /ipc/share and /ipc/send always answer 200 with plain text, so success is
    the "SUCCESS" prefix, not the status code.
  - search, branch, diff, merge, and raptor_build use real status codes
    (401 unknown agent, 403 denied pipe, 400 other errors).
"""

import json
import re
import sys
import time
from pathlib import Path

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from ore_client import OreClient, get_ore_dir  # noqa: E402

JSON = {"Accept": "application/json"}


class KernelError(RuntimeError):
    """The kernel refused or failed a request."""

    def __init__(self, route: str, status: int, body: str):
        super().__init__(f"{route} -> {status}: {body.strip()[:300]}")
        self.route = route
        self.status = status
        self.body = body


class OreMmuClient(OreClient):
    def __init__(self, base_url: str = None, timeout: float = 600.0):
        super().__init__(base_url)
        self.timeout = timeout
        self.session = requests.Session()
        self.session.headers.update(self.headers)

    # ─── Transport ───────────────────────────────────────────────

    def _send(self, method: str, route: str, **kw) -> requests.Response:
        r = self.session.request(method, f"{self.base_url}{route}", timeout=self.timeout, **kw)
        # The kernel writes a new session token every time it boots. If it restarted
        # mid-run, pick up the new token and retry once. Only when the token on disk
        # actually changed, so a deliberate 401 (unknown agent) is never retried.
        if r.status_code == 401 and self._reload_token():
            r = self.session.request(method, f"{self.base_url}{route}", timeout=self.timeout, **kw)
        return r

    def _reload_token(self) -> bool:
        fresh = self._read_token()
        if fresh == self.token:
            return False
        self.token = fresh
        self.headers["Authorization"] = f"Bearer {fresh}"
        self.session.headers["Authorization"] = f"Bearer {fresh}"
        return True

    def _post(self, route: str, payload: dict, headers: dict = None) -> requests.Response:
        return self._send("POST", route, json=payload, headers=headers)

    def _get(self, route: str, headers: dict = None) -> requests.Response:
        return self._send("GET", route, headers=headers)

    @staticmethod
    def _ok(route: str, r: requests.Response) -> requests.Response:
        if r.status_code != 200:
            raise KernelError(route, r.status_code, r.text)
        return r

    # ─── System ──────────────────────────────────────────────────

    def health_json(self) -> dict:
        return self._ok("/health", self._get("/health", JSON)).json()

    def agents_json(self) -> list:
        return self._ok("/agents", self._get("/agents", JSON)).json().get("agents", [])

    def models_json(self) -> list:
        return self._ok("/ls", self._get("/ls", JSON)).json().get("models", [])

    def has_model(self, model: str) -> bool:
        wanted = model.lower().replace(":", "-")
        return any(m["name"].lower().replace(":", "-") == wanted for m in self.models_json())

    # ─── Semantic Bus: write ─────────────────────────────────────

    def share_raw(
        self,
        app: str,
        pipe: str,
        text: str,
        *,
        category: str = None,
        chunk_strategy: str = None,
        chunk_size: int = None,
        chunk_overlap: int = None,
    ) -> str:
        """POST /ipc/share. Returns the kernel's text verbatim (success or denial)."""
        return self._share(app, pipe, text, category, chunk_strategy, chunk_size, chunk_overlap).text

    def _share(self, app, pipe, text, category=None, chunk_strategy=None, chunk_size=None, chunk_overlap=None):
        payload = {"source_app": app, "target_pipe": pipe, "knowledge_text": text}
        if category:
            payload["category"] = category
        if chunk_strategy:
            payload["chunk_strategy"] = chunk_strategy
        if chunk_size is not None:
            payload["chunk_size"] = chunk_size
        if chunk_overlap is not None:
            payload["chunk_overlap"] = chunk_overlap
        return self._post("/ipc/share", payload)

    def share(self, app: str, pipe: str, text: str, **kw) -> str:
        """POST /ipc/share. Raises KernelError unless the kernel answers SUCCESS."""
        r = self._share(app, pipe, text, **kw)
        if r.status_code != 200 or not r.text.startswith("SUCCESS"):
            raise KernelError("/ipc/share", r.status_code, r.text or "(empty body)")
        return r.text

    def share_docs(self, app: str, pipe: str, docs: list, category: str = None, batch: int = 500) -> int:
        """Store each doc as exactly one chunk, in as few requests as possible.

        The `paragraph` strategy packs paragraphs while they fit in chunk_size
        words. With chunk_size = the longest doc and every doc longer than half
        of that, no two docs fit together, so each doc becomes its own chunk.
        Docs are sorted by length and grouped so that holds; anything that
        can't be grouped is sent alone with `exact_match`.
        """
        docs = [d.strip() for d in docs if d.strip()]
        ordered = sorted(docs, key=lambda d: len(d.split()))
        group = []

        def flush():
            if not group:
                return
            if len(group) == 1:
                self.share(app, pipe, group[0], category=category, chunk_strategy="exact_match")
            else:
                longest = max(len(d.split()) for d in group)
                self.share(
                    app,
                    pipe,
                    "\n\n".join(group),
                    category=category,
                    chunk_strategy="paragraph",
                    chunk_size=longest,
                    chunk_overlap=0,
                )
            group.clear()

        for doc in ordered:
            if "\n\n" in doc:
                flush()
                self.share(app, pipe, doc, category=category, chunk_strategy="exact_match")
                continue
            words = len(doc.split())
            shortest = len(group[0].split()) if group else words
            if group and (shortest * 2 <= words or len(group) >= batch):
                flush()
            group.append(doc)
        flush()
        return len(docs)

    # ─── Semantic Bus: read ──────────────────────────────────────

    def search_raw(
        self,
        app: str,
        pipe: str,
        query: str,
        *,
        top_k: int = 5,
        filter_app: str = None,
        filter_category: str = None,
        enable_hybrid: bool = None,
        enable_hyde: bool = None,
        confidence_threshold: float = None,
        crag_fallback_tool: str = None,
    ) -> requests.Response:
        payload = {"source_app": app, "target_pipe": pipe, "query": query, "top_k": top_k}
        optional = {
            "filter_app": filter_app,
            "filter_category": filter_category,
            "enable_hybrid": enable_hybrid,
            "enable_hyde": enable_hyde,
            "confidence_threshold": confidence_threshold,
            "crag_fallback_tool": crag_fallback_tool,
        }
        payload.update({k: v for k, v in optional.items() if v is not None})
        return self._post("/ipc/search", payload)

    def search(self, app: str, pipe: str, query: str, **kw) -> list:
        """POST /ipc/search. Returns the list of SearchResult dicts."""
        return self._ok("/ipc/search", self.search_raw(app, pipe, query, **kw)).json()

    # ─── CoW branches ────────────────────────────────────────────

    def branch_raw(self, app: str, base: str, branch: str) -> requests.Response:
        return self._post("/ipc/pipe/branch", {"app_id": app, "base_pipe": base, "branch_pipe": branch})

    def branch(self, app: str, base: str, branch: str) -> str:
        return self._ok("/ipc/pipe/branch", self.branch_raw(app, base, branch)).text

    def diff_raw(self, app: str, base: str, branch: str) -> requests.Response:
        return self._post("/ipc/pipe/diff", {"app_id": app, "base_pipe": base, "branch_pipe": branch})

    def diff(self, app: str, base: str, branch: str) -> dict:
        return self._ok("/ipc/pipe/diff", self.diff_raw(app, base, branch)).json()

    def merge_raw(self, app: str, base: str, branch: str, delete: bool = False) -> requests.Response:
        return self._post(
            "/ipc/pipe/merge",
            {
                "app_id": app,
                "base_pipe": base,
                "branch_pipe": branch,
                "delete_branch_after_merge": delete,
            },
        )

    def merge(self, app: str, base: str, branch: str, delete: bool = False) -> int:
        """Returns the number of chunks merged."""
        body = self._ok("/ipc/pipe/merge", self.merge_raw(app, base, branch, delete)).text
        m = re.search(r"Merged (\d+) chunks", body)
        return int(m.group(1)) if m else 0

    def raptor_build(self, app: str, pipe: str, cluster_size: int = None, max_levels: int = None) -> dict:
        payload = {"app_id": app, "target_pipe": pipe}
        if cluster_size is not None:
            payload["cluster_size"] = cluster_size
        if max_levels is not None:
            payload["max_levels"] = max_levels
        return self._ok("/ipc/pipe/raptor_build", self._post("/ipc/pipe/raptor_build", payload)).json()

    # ─── Message Bus ─────────────────────────────────────────────

    def send(self, from_app: str, to_app: str, payload) -> str:
        """POST /ipc/send. Returns the kernel's text verbatim. Dicts are sent as JSON."""
        if not isinstance(payload, str):
            payload = json.dumps(payload)
        r = self._post(
            "/ipc/send",
            {"from_app": from_app, "to_app": to_app, "payload": payload, "timestamp": int(time.time())},
        )
        return r.text

    def listen(self, app_id: str):
        """GET /ipc/listen/:app_id. Returns the next message dict, or None."""
        return self._ok("/ipc/listen", self._get(f"/ipc/listen/{app_id}")).json()

    def drain(self, app_id: str, limit: int = 100_000) -> list:
        out = []
        while len(out) < limit:
            msg = self.listen(app_id)
            if msg is None:
                break
            out.append(msg)
        return out

    # ─── Inference ───────────────────────────────────────────────

    def infer(self, app_id: str, prompt: str) -> requests.Response:
        """POST /infer as `app_id` (firewall + Auto-RAG + paging)."""
        return self._post("/infer", {"prompt": prompt}, headers={"X-App-ID": app_id})


__all__ = ["OreMmuClient", "KernelError", "get_ore_dir"]
