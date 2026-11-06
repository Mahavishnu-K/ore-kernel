"""s11 - Features that need a model or a tool: HyDE, RAPTOR, Auto-RAG, CRAG.

HyDE, RAPTOR, and Auto-RAG need the analyst's model (qwen2.5:0.5b) on disk;
they SKIP otherwise, or with --skip-llm. CRAG needs no model, only a WASM tool
that prints findings for argv[1]: pass it with --crag-tool <name>.
"""

import time

import corpus
from harness import Skip, scenario

from .common import ANALYST, CRAG, REVIEWER, rank_of


@scenario("s11_hyde", "HyDE: search with a hypothetical answer", needs_llm=True)
def hyde(ctx):
    c = ctx.client
    pipe = ctx.pipe("mmu_kb", "s11_hyde")
    c.share_docs(REVIEWER, pipe, [d.text for d in corpus.CORE_DOCS])

    query = "Why did the phone app start crashing?"
    expected = corpus.DOC_BY_ID["f09"].text

    start = time.perf_counter()
    plain = c.search(ANALYST, pipe, query, top_k=3, enable_hyde=False)
    plain_ms = (time.perf_counter() - start) * 1000
    start = time.perf_counter()
    hyde_results = c.search(ANALYST, pipe, query, top_k=3, enable_hyde=True)
    hyde_ms = (time.perf_counter() - start) * 1000

    ctx.expect(hyde_results, "HyDE search returned nothing")
    ctx.metric("plain_rank", rank_of(plain, expected))
    ctx.metric("hyde_rank", rank_of(hyde_results, expected))
    ctx.metric("plain_ms", round(plain_ms, 1))
    ctx.metric("hyde_ms", round(hyde_ms, 1))
    ctx.expect(rank_of(hyde_results, expected) is not None, "HyDE lost the answer from the top 3")


@scenario("s11_raptor", "RAPTOR: build a summary tree over a pipe", needs_llm=True)
def raptor(ctx):
    c = ctx.client
    pipe = ctx.pipe("mmu_kb", "s11_raptor")
    leaves = [d.text for d in corpus.docs_in("fact")]
    c.share_docs(REVIEWER, pipe, leaves, category="fact")

    start = time.perf_counter()
    built = c.raptor_build(ANALYST, pipe, cluster_size=4, max_levels=2)
    ctx.metric("build_s", round(time.perf_counter() - start, 2))
    ctx.metric("generated_summaries", built["generated_summaries"])
    ctx.metric("tree_levels", built["tree_levels"])

    ctx.expect(built["target_pipe"] == pipe, f"wrong target pipe in response: {built}")
    ctx.expect(built["tree_levels"] >= 1, f"no tree levels were built: {built}")
    ctx.expect(built["generated_summaries"] >= len(leaves) // 4, f"too few summaries for {len(leaves)} leaves: {built}")

    summaries = c.search(ANALYST, pipe, "summary of recent incidents and releases", top_k=10, filter_app="raptor_l1")
    ctx.expect(summaries, "no level-1 summary chunks are searchable")
    ctx.expect(all(s["source_app"] == "raptor_l1" for s in summaries), "filter_app leaked non-summary chunks")
    ctx.expect(all(s["text"] not in leaves for s in summaries), "a summary is just a copy of a leaf")


@scenario("s11_auto_rag", "Auto-RAG: POST /infer answers from shared memory", needs_llm=True)
def auto_rag(ctx):
    c = ctx.client
    # Auto-RAG reads the analyst's first non-wildcard pipe: mmu_rag_kb.
    codename = f"cobalt-heron-{ctx.run_id[-4:]}"
    fact = f"The Lighthouse rollout for run {ctx.run_id} uses the codename {codename}."
    c.share(ANALYST, "mmu_rag_kb", fact, category="fact", chunk_strategy="exact_match")

    start = time.perf_counter()
    r = c.infer(ANALYST, f"What codename does the Lighthouse rollout for run {ctx.run_id} use? Answer with the codename only.")
    ctx.metric("infer_s", round(time.perf_counter() - start, 2))
    ctx.expect(r.status_code == 200, f"/infer -> {r.status_code}: {r.text[:200]}")
    ctx.log(f"answer: {r.text.strip()[:120]}")
    ctx.expect(codename.lower() in r.text.lower(), f"answer doesn't contain the planted codename {codename}: {r.text.strip()[:160]}")


@scenario("s11_crag", "CRAG: low confidence triggers a sandboxed fallback tool")
def crag(ctx):
    tool = getattr(ctx.args, "crag_tool", None)
    if not tool:
        raise Skip("pass --crag-tool <name> (a WASM tool in tools/ that prints findings for argv[1])")

    c = ctx.client
    pipe = ctx.pipe("mmu_crag", "s11")
    c.share(CRAG, pipe, corpus.DOC_BY_ID["g02"].text, chunk_strategy="exact_match")

    results = c.search(
        CRAG,
        pipe,
        "latest status of the Lighthouse rollout",
        top_k=5,
        confidence_threshold=0.99,
        crag_fallback_tool=tool,
    )
    rescued = [r for r in results if r["is_crag_fallback"]]
    ctx.metric("fallback_chunks", len(rescued))
    ctx.expect(rescued, f"no CRAG fallback result: tool '{tool}' failed, printed nothing, or isn't in mmu_crag's allowed_tools")
    ctx.expect("KERNEL ERROR" not in rescued[0]["text"], "a failed tool run was stored as knowledge")
    ctx.expect(rescued[0]["category"] == "fact", "CRAG findings are stored in the fact tier")
