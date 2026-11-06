"""s12 - Web search for agents through POST /net/fetch, and CRAG on the agent side.

Part 1 needs no internet: the kernel refuses these requests before opening a
connection, using only the agent's [network] manifest rules.
Part 2 searches Wikipedia and DuckDuckGo for real; it SKIPs when the machine
is offline, and FAILs when the firewall blocks a request it should allow.
"""

import time

import corpus
import requests
from harness import Skip, scenario
from web_search import WebSearch, crag_search

from .common import GHOST, OUTSIDER, WEB


def fetch_status(client, app_id: str, url: str, method: str = "GET") -> int:
    try:
        client.net_fetch(app_id, url, method=method)
        return 200
    except requests.HTTPError as e:
        return e.response.status_code


@scenario("s12_net_firewall", "/net/fetch: manifest egress rules refuse what isn't allowed")
def net_firewall(ctx):
    c = ctx.client
    cases = [
        (WEB, "GET", "https://example.com/", 403, "domain not in [[network.rules]]"),
        (WEB, "GET", "https://en.wikipedia.org/wiki/Rust", 403, "path outside allowed_paths"),
        (WEB, "POST", "https://en.wikipedia.org/w/api.php", 403, "method outside allowed_methods"),
        (WEB, "GET", f"{c.base_url}/health", 403, "the kernel's own API (localhost)"),
        (WEB, "GET", "http://169.254.169.254/latest/meta-data/", 403, "cloud metadata address"),
        (WEB, "GET", "file:///etc/passwd", 403, "non-HTTP scheme"),
        (OUTSIDER, "GET", "https://en.wikipedia.org/w/api.php", 403, "agent without a [network] section"),
        (GHOST, "GET", "https://en.wikipedia.org/w/api.php", 401, "unregistered agent"),
    ]
    for app, method, url, expected, why in cases:
        got = fetch_status(c, app, url, method)
        ctx.expect(got == expected, f"{app} {method} {url} ({why}): expected {expected}, got {got}")
    ctx.metric("refusals_checked", len(cases))


@scenario("s12_web_search", "Web search via /net/fetch + agent-side CRAG into memory")
def web_search(ctx):
    c = ctx.client
    ws = WebSearch(c, WEB)

    out = ws.search("Rust programming language", max_results=5)
    for p in out.providers:
        ctx.metric(f"{p.name}_ms", p.elapsed_ms)
        ctx.log(f"{p.name}: ok={p.ok} results={p.results} {p.error}")
    if out.blocked:
        raise AssertionError("firewall blocked an allowed search API: " + "; ".join(p.error for p in out.providers if p.blocked))
    if out.all_failed:
        raise Skip("no provider reachable (offline?): " + "; ".join(p.error for p in out.providers))

    ctx.expect(out.results, "search returned no results")
    ctx.expect(all(r.url.startswith("https://") and r.title and r.snippet for r in out.results), "incomplete result")
    ctx.expect(any("rust" in r.snippet.lower() for r in out.results), "results don't mention the query topic")
    ctx.metric("results", len(out.results))

    # Agent-side CRAG: the pipe only knows about the office coffee machine.
    pipe = ctx.pipe("mmu_web", "crag")
    c.share(WEB, pipe, corpus.DOC_BY_ID["g02"].text, chunk_strategy="exact_match")
    query = "Rust programming language"

    start = time.perf_counter()
    results, used_web = crag_search(c, ws, pipe, query, threshold=0.6)
    ctx.metric("crag_cold_s", round(time.perf_counter() - start, 2))
    ctx.expect(used_web, "low-confidence retrieval should have fallen back to the web")
    ctx.expect(results and "Source:" in results[0]["text"], f"top result isn't a web finding: {results[:1]}")
    ctx.expect(results[0]["source_app"] == WEB and results[0]["category"] == "fact", "findings must be facts owned by mmu_web")

    start = time.perf_counter()
    again, used_web_again = crag_search(c, ws, pipe, query, threshold=0.6)
    ctx.metric("crag_warm_s", round(time.perf_counter() - start, 2))
    ctx.expect(not used_web_again, f"second ask should come from memory (top score {again[0]['score'] if again else None})")
