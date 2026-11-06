"""
web_search.py - Web search for Ore agents, through the kernel's egress firewall.

Every HTTP request goes through POST /net/fetch (OreClient.net_fetch), so the
kernel applies the calling agent's [network] manifest rules: allowed domain,
method, and path, no localhost or link-local addresses, and a re-check of every
redirect hop. The agent process never opens a socket of its own.

Providers (no API keys):
  - Wikipedia: MediaWiki search with plain-text intro extracts
  - DuckDuckGo Instant Answers: direct answers, abstracts, definitions, related topics

Use it as a library:

    from web_search import WebSearch, crag_search
    ws = WebSearch(client, "mmu_web")
    out = ws.search("Rust programming language")
    print(out.to_text())

    # CRAG on the agent side: answer from memory, fall back to the web when unsure
    results, used_web = crag_search(client, ws, "mmu_web_kb", "Who created Rust?")

Or from the command line:

    python web_search.py "Rust programming language"
    python web_search.py --json --max 3 --provider wikipedia "Dennis Ritchie"
    python web_search.py --ingest mmu_web_notes "WebAssembly System Interface"

The agent's manifest needs (see manifests/mmu_web.toml):

    [network]
    network_enabled = true
    allow_localhost_access = false

    [[network.rules]]
    domain = "en.wikipedia.org"
    allowed_methods = ["GET"]
    allowed_paths = ["/w/api.php"]

    [[network.rules]]
    domain = "api.duckduckgo.com"
    allowed_methods = ["GET"]
    allowed_paths = ["/"]
"""

import argparse
import html
import json
import re
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote, urlencode, urlsplit, urlunsplit

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mmu_client import OreMmuClient  # noqa: E402

VERSION = "1.0.0"
# Wikimedia refuses API requests without a descriptive User-Agent (HTTP 403).
USER_AGENT = f"ore-web-search/{VERSION} (Ore kernel agent; https://github.com/Mahavishnu-K/ore-kernel)"
DEFAULT_APP = "mmu_web"
MAX_RESULTS = 10
MAX_QUERY_CHARS = 300
SNIPPET_CHARS = 320

WIKIPEDIA = "wikipedia"
DUCKDUCKGO = "duckduckgo"
PROVIDERS = (DUCKDUCKGO, WIKIPEDIA)

# Kernel statuses worth retrying: 502 means the upstream failed, not the policy.
RETRYABLE_KERNEL = {502}
RETRYABLE_UPSTREAM = {429, 500, 502, 503, 504}

_LANG = re.compile(r"^[a-z]{2,3}(-[a-z]+)?$")
_SPACE = re.compile(r"\s+")


# ─── Errors and results ──────────────────────────────────────────


class WebSearchError(RuntimeError):
    """A provider failed. `blocked` is True when the kernel's firewall refused it."""

    def __init__(self, provider: str, message: str, blocked: bool = False, offline: bool = False):
        super().__init__(f"{provider}: {message}")
        self.provider = provider
        self.blocked = blocked
        self.offline = offline


@dataclass
class SearchResult:
    title: str
    snippet: str
    url: str
    source: str

    def as_memory(self) -> str:
        """One self-contained sentence block, for storing as a Semantic Bus chunk."""
        return f"{self.title}: {self.snippet} (Source: {self.source}, {self.url})"


@dataclass
class ProviderReport:
    name: str
    ok: bool
    results: int = 0
    error: str = ""
    blocked: bool = False
    offline: bool = False
    elapsed_ms: float = 0.0


@dataclass
class SearchOutput:
    query: str
    retrieved_at: str
    results: list = field(default_factory=list)
    providers: list = field(default_factory=list)

    @property
    def all_failed(self) -> bool:
        return bool(self.providers) and not any(p.ok for p in self.providers)

    @property
    def blocked(self) -> bool:
        return any(p.blocked for p in self.providers)

    def to_text(self) -> str:
        lines = [f'Web search results for "{self.query}" (retrieved {self.retrieved_at}):']
        for i, r in enumerate(self.results, 1):
            lines.append(f"{i}. {r.title}: {r.snippet}")
            lines.append(f"   Source: {r.source}, {r.url}")
        return "\n".join(lines)

    def to_dict(self) -> dict:
        return {
            "query": self.query,
            "retrieved_at": self.retrieved_at,
            "providers": [asdict(p) for p in self.providers],
            "results": [asdict(r) for r in self.results],
        }


# ─── Text helpers ────────────────────────────────────────────────


def clean(text) -> str:
    if not isinstance(text, str):
        return ""
    return _SPACE.sub(" ", html.unescape(text)).strip()


def truncate(text: str, limit: int = SNIPPET_CHARS) -> str:
    if len(text) <= limit:
        return text
    cut = text[:limit]
    space = cut.rfind(" ")
    if space > limit // 2:
        cut = cut[:space]
    return cut.rstrip(" ,;:") + "..."


def canonical_url(url: str) -> str:
    try:
        parts = urlsplit(url.strip())
    except ValueError:
        return url.strip().lower()
    return urlunsplit(("https", parts.netloc.lower(), parts.path.rstrip("/"), parts.query, ""))


def dedupe(results: list, limit: int) -> list:
    seen_urls, seen_text, out = set(), set(), []
    for r in results:
        key_url, key_text = canonical_url(r.url), r.snippet.lower()
        if key_url in seen_urls or key_text in seen_text:
            continue
        seen_urls.add(key_url)
        seen_text.add(key_text)
        out.append(r)
        if len(out) == limit:
            break
    return out


def normalize_query(query: str) -> str:
    q = clean(query)[:MAX_QUERY_CHARS].strip()
    if not q:
        raise ValueError("empty query")
    return q


# ─── Search client ───────────────────────────────────────────────


class WebSearch:
    def __init__(self, client, app_id: str = DEFAULT_APP, lang: str = "en", retries: int = 1, backoff_s: float = 0.5):
        if not _LANG.match(lang):
            raise ValueError(f"invalid Wikipedia language code: {lang!r}")
        self.client = client
        self.app_id = app_id
        self.lang = lang
        self.retries = retries
        self.backoff_s = backoff_s

    # ─── Transport ───────────────────────────────────────────────

    def _get_json(self, provider: str, url: str) -> dict:
        """GET url through /net/fetch and decode JSON. Retries upstream failures, never firewall refusals."""
        headers = {"User-Agent": USER_AGENT, "Accept": "application/json", "Accept-Language": self.lang}
        last = None
        for attempt in range(self.retries + 1):
            if attempt:
                time.sleep(self.backoff_s * attempt)
            try:
                resp = self.client.net_fetch(self.app_id, url, method="GET", headers=headers)
            except requests.HTTPError as e:
                status = e.response.status_code if e.response is not None else 0
                detail = (e.response.text if e.response is not None else str(e)).strip()[:300]
                if status in (401, 403):
                    raise WebSearchError(provider, f"refused by the kernel ({status}): {detail}", blocked=True) from e
                if status in RETRYABLE_KERNEL:
                    last = WebSearchError(provider, f"upstream unreachable: {detail}", offline=True)
                    continue
                raise WebSearchError(provider, f"kernel error {status}: {detail}") from e
            except requests.ConnectionError as e:
                raise WebSearchError(provider, f"cannot reach the Ore kernel: {e}") from e

            status = resp.get("status", 0)
            if status in RETRYABLE_UPSTREAM:
                last = WebSearchError(provider, f"upstream HTTP {status}")
                continue
            if status != 200:
                raise WebSearchError(provider, f"upstream HTTP {status} {resp.get('status_text', '')}".strip())
            try:
                return json.loads(resp.get("body") or "")
            except json.JSONDecodeError as e:
                raise WebSearchError(provider, f"invalid JSON from {urlsplit(url).netloc}: {e}") from e
        raise last

    # ─── Providers ───────────────────────────────────────────────

    def wikipedia(self, query: str, limit: int) -> list:
        params = {
            "action": "query",
            "format": "json",
            "formatversion": "2",
            "generator": "search",
            "gsrsearch": query,
            "gsrlimit": limit,
            "gsrnamespace": 0,
            "prop": "extracts|info",
            "exintro": 1,
            "explaintext": 1,
            "exsentences": 3,
            "exlimit": limit,
            "inprop": "url",
            "redirects": 1,
            "utf8": 1,
            "maxlag": 5,
        }
        data = self._get_json(WIKIPEDIA, f"https://{self.lang}.wikipedia.org/w/api.php?{urlencode(params)}")
        if "error" in data:
            err = data["error"]
            raise WebSearchError(WIKIPEDIA, f"MediaWiki API error {err.get('code')}: {err.get('info')}")

        pages = sorted(data.get("query", {}).get("pages", []), key=lambda p: p.get("index", 0))
        results = []
        for p in pages:
            title, extract = clean(p.get("title")), clean(p.get("extract"))
            if p.get("missing") or not title or not extract:
                continue  # empty intros (disambiguation stubs) carry nothing useful
            url = p.get("fullurl") or f"https://{self.lang}.wikipedia.org/wiki/{quote(title.replace(' ', '_'))}"
            results.append(SearchResult(title, truncate(extract), url, WIKIPEDIA))
        return results

    def duckduckgo(self, query: str) -> tuple:
        """Returns (direct, related): direct answers rank above everything else."""
        params = {"q": query, "format": "json", "no_html": 1, "no_redirect": 1, "skip_disambig": 1, "t": "ore-web-search"}
        data = self._get_json(DUCKDUCKGO, f"https://api.duckduckgo.com/?{urlencode(params)}")

        search_url = f"https://duckduckgo.com/?q={quote(query)}"
        heading = clean(data.get("Heading")) or query
        direct, related = [], []

        answer = clean(data.get("Answer"))  # sometimes an object; clean() drops non-strings
        if answer:
            direct.append(SearchResult(heading, truncate(answer), data.get("AbstractURL") or search_url, DUCKDUCKGO))
        abstract = clean(data.get("AbstractText"))
        if abstract:
            source = clean(data.get("AbstractSource"))
            label = f"{DUCKDUCKGO} ({source})" if source else DUCKDUCKGO
            direct.append(SearchResult(heading, truncate(abstract), data.get("AbstractURL") or search_url, label))
        definition = clean(data.get("Definition"))
        if definition:
            direct.append(
                SearchResult(f"{heading} (definition)", truncate(definition), data.get("DefinitionURL") or search_url, DUCKDUCKGO)
            )

        def walk(topics):
            for t in topics or []:
                if t.get("Topics"):
                    walk(t["Topics"])
                    continue
                text, url = clean(t.get("Text")), t.get("FirstURL")
                if not text or not url:
                    continue
                title, _, rest = text.partition(" - ")
                if not rest or len(title) > 120:
                    title, rest = truncate(text, 80), text
                related.append(SearchResult(title, truncate(rest), url, DUCKDUCKGO))

        walk(data.get("Results"))
        walk(data.get("RelatedTopics"))
        return direct, related

    # ─── Search ──────────────────────────────────────────────────

    def search(self, query: str, max_results: int = 5, providers: tuple = PROVIDERS) -> SearchOutput:
        """Queries each provider; one failing provider doesn't fail the search.

        Ranking: DuckDuckGo direct answers, then Wikipedia articles, then related topics.
        """
        if not 1 <= max_results <= MAX_RESULTS:
            raise ValueError(f"max_results must be between 1 and {MAX_RESULTS}")
        unknown = set(providers) - set(PROVIDERS)
        if unknown:
            raise ValueError(f"unknown providers: {sorted(unknown)}")
        query = normalize_query(query)

        out = SearchOutput(query=query, retrieved_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
        direct, wiki, related = [], [], []
        for name in providers:
            start = time.perf_counter()
            try:
                if name == DUCKDUCKGO:
                    direct, related = self.duckduckgo(query)
                    count = len(direct) + len(related)
                else:
                    wiki = self.wikipedia(query, max_results)
                    count = len(wiki)
                report = ProviderReport(name, True, count)
            except WebSearchError as e:
                report = ProviderReport(name, False, error=str(e), blocked=e.blocked, offline=e.offline)
            report.elapsed_ms = round((time.perf_counter() - start) * 1000, 1)
            out.providers.append(report)

        out.results = dedupe(direct + wiki + related, max_results)
        return out

    def ingest(self, pipe: str, output: SearchOutput, category: str = "fact") -> int:
        """Stores each result as its own chunk in `pipe` (the agent needs that pipe in its manifest)."""
        for r in output.results:
            self.client.share(self.app_id, pipe, r.as_memory(), category=category, chunk_strategy="exact_match")
        return len(output.results)


# ─── CRAG on the agent side ──────────────────────────────────────


def crag_search(client, ws: WebSearch, pipe: str, query: str, threshold: float = 0.55, top_k: int = 5) -> tuple:
    """Answer from shared memory; when the best match is below `threshold`, search
    the web, store the findings in `pipe`, and search again.

    Returns (results, used_web). Confidence is the dense (cosine) score: the
    hybrid RRF score gives the top result 1.0 whenever only one retriever has
    hits, so it can't tell a relevant match from the only match.
    """
    # An empty or never-written pipe returns [], which counts as low confidence.
    results = client.search(ws.app_id, pipe, query, top_k=top_k, enable_hybrid=False)
    if results and results[0]["score"] >= threshold:
        return results, False

    found = ws.search(query, max_results=top_k)
    if found.all_failed:
        raise WebSearchError("web", "; ".join(p.error for p in found.providers), blocked=found.blocked)
    if not found.results:
        return results, False
    ws.ingest(pipe, found)
    return client.search(ws.app_id, pipe, query, top_k=top_k, enable_hybrid=False), True


# ─── Command line ────────────────────────────────────────────────


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description="Web search through the Ore kernel's egress firewall (POST /net/fetch)")
    p.add_argument("query", nargs="*", help="search terms (read from stdin when omitted)")
    p.add_argument("--app", default=DEFAULT_APP, help=f"agent identity whose [network] rules apply (default {DEFAULT_APP})")
    p.add_argument("--max", type=int, default=5, help=f"results to return, 1-{MAX_RESULTS} (default 5)")
    p.add_argument("--provider", choices=["auto", WIKIPEDIA, DUCKDUCKGO], default="auto")
    p.add_argument("--lang", default="en", help="Wikipedia language code (default en)")
    p.add_argument("--json", action="store_true", help="print JSON instead of text")
    p.add_argument("--ingest", metavar="PIPE", help="also store the results in this Semantic Bus pipe")
    p.add_argument("-v", "--verbose", action="store_true", help="report failed providers even when others succeed")
    args = p.parse_args(argv)
    # Titles can be in any script; don't crash on a legacy Windows console code page.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(errors="replace")

    query = " ".join(args.query) or sys.stdin.read()
    providers = PROVIDERS if args.provider == "auto" else (args.provider,)
    try:
        ws = WebSearch(OreMmuClient(), args.app, lang=args.lang)
        out = ws.search(query, max_results=args.max, providers=providers)
    except ValueError as e:
        print(f"web_search: {e}", file=sys.stderr)
        return 1

    if out.all_failed:
        for r in out.providers:
            print(f"web_search: {r.error}", file=sys.stderr)
        return 2
    if args.verbose:
        for r in out.providers:
            if not r.ok:
                print(f"web_search: {r.error}", file=sys.stderr)
    if not out.results:
        print(f'web_search: no results for "{out.query}"', file=sys.stderr)
        return 3

    if args.ingest:
        stored = ws.ingest(args.ingest, out)
        print(f"web_search: stored {stored} results in '{args.ingest}'", file=sys.stderr)
    print(json.dumps(out.to_dict(), indent=2, ensure_ascii=False) if args.json else out.to_text())
    return 0


if __name__ == "__main__":
    sys.exit(main())
