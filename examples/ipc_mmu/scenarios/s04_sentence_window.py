"""s04 - Sentence-window chunking: match on one sentence, return its neighbours."""

import corpus
from harness import scenario

from .common import RESEARCHER


@scenario("s04_sentence_window", "Sentence-window: precise probe, wide context_payload")
def run(ctx):
    c = ctx.client
    window_pipe = ctx.pipe("mmu_facts", "s04_window")
    sliding_pipe = ctx.pipe("mmu_facts", "s04_sliding")

    c.share(RESEARCHER, window_pipe, corpus.SENTENCE_WINDOW_TEXT, chunk_strategy="sentence_window", chunk_size=40)
    c.share(RESEARCHER, sliding_pipe, corpus.SENTENCE_WINDOW_TEXT, chunk_strategy="sliding_window", chunk_size=40, chunk_overlap=8)

    top = c.search(RESEARCHER, window_pipe, corpus.SENTENCE_WINDOW_QUERY, top_k=1)
    ctx.expect(top, "sentence-window pipe returned nothing")
    hit = top[0]
    ctx.expect(hit["text"] == corpus.SENTENCE_WINDOW_ANSWER, f"probe should be the single answering sentence, got: {hit['text']}")
    ctx.expect(hit["text"] in hit["context_payload"], "context_payload must contain the probe sentence")
    ctx.expect(
        len(hit["context_payload"].split()) > len(hit["text"].split()),
        "context_payload should carry neighbouring sentences",
    )

    every = c.search(RESEARCHER, window_pipe, "Atlas cluster", top_k=20)
    sentences = corpus.SENTENCE_WINDOW_TEXT.count(". ") + 1
    ctx.expect(len(every) == sentences, f"expected one chunk per sentence ({sentences}), found {len(every)}")

    plain = c.search(RESEARCHER, sliding_pipe, corpus.SENTENCE_WINDOW_QUERY, top_k=1)
    ctx.expect(plain and plain[0]["text"] == plain[0]["context_payload"], "sliding_window chunks are their own payload")

    ctx.metric("probe_words", len(hit["text"].split()))
    ctx.metric("payload_words", len(hit["context_payload"].split()))
