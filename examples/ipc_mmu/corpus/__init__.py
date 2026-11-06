"""
corpus - Labelled, offline evaluation data for the Cognitive MMU suite.

  core.py         24 operations-team documents, 16 queries (exact IDs, paraphrases)
  near_miss.py    20 look-alike documents and queries (ranking precision)
  multi_doc.py    6 linking documents, 12 questions needing 2-3 documents
  technical.py    20 stack traces, configs, addresses, hashes; 18 queries
  cjk.py          9 Chinese, Japanese, Korean, French, Russian documents; 9 queries
  long_docs.py    10 postmortems, runbooks, design and review docs (~4,700 words) stored
                  chunked; 32 queries by answer phrase
  unanswerable.py 25 questions with no answer (CRAG threshold calibration)
  adversarial.*   26 firewall cases: injections, evasions, PII, false positives
  fixtures.py     sentence-window paragraph, swarm tasks
  synthetic.py    bulk records for benchmarks

Everything is deterministic and needs no network.

CORE_DOCS / CORE_QUERIES are the original set the behaviour scenarios rely on.
EVAL_DOCS / EVAL_QUERIES are the full retrieval benchmark.
"""

from . import adversarial, cjk, core, long_docs, multi_doc, near_miss, technical, unanswerable
from .fixtures import POISON_TEXT, SENTENCE_WINDOW_ANSWER, SENTENCE_WINDOW_QUERY, SENTENCE_WINDOW_TEXT, SWARM_TASKS
from .model import KINDS, Doc, LongDoc, Query
from .synthetic import synthetic_doc, synthetic_docs

CORE_DOCS = list(core.DOCS)
CORE_QUERIES = list(core.QUERIES)

# Documents stored whole, one chunk each.
EVAL_DOCS = CORE_DOCS + near_miss.DOCS + multi_doc.DOCS + technical.DOCS + cjk.DOCS
# Documents stored chunked.
LONG_DOCS = list(long_docs.DOCS)

EVAL_QUERIES = (
    CORE_QUERIES
    + near_miss.QUERIES
    + multi_doc.QUERIES
    + technical.QUERIES
    + long_docs.QUERIES
    + cjk.QUERIES
    + unanswerable.QUERIES
)

DOC_BY_ID = {d.id: d for d in EVAL_DOCS}
ID_BY_TEXT = {d.text: d.id for d in EVAL_DOCS}


def docs_in(category: str, docs: list = None) -> list:
    """Documents of one tier; the core set unless `docs` is given."""
    return [d for d in (CORE_DOCS if docs is None else docs) if d.category == category]


def queries_of(kind: str) -> list:
    return [q for q in EVAL_QUERIES if q.kind == kind]


def _validate():
    """Fails at import if a label points at nothing, so a typo can't skew metrics."""
    ids = [d.id for d in EVAL_DOCS] + [d.id for d in LONG_DOCS]
    dupes = {i for i in ids if ids.count(i) > 1}
    assert not dupes, f"duplicate doc ids: {dupes}"
    qids = [q.id for q in EVAL_QUERIES]
    assert len(qids) == len(set(qids)), "duplicate query ids"
    long_text = " ".join(d.text for d in LONG_DOCS).lower()
    for q in EVAL_QUERIES:
        assert q.kind in KINDS, f"{q.id}: unknown kind {q.kind}"
        for rid in q.relevant:
            assert rid in DOC_BY_ID, f"{q.id}: unknown relevant doc {rid}"
        for phrase in q.answers:
            assert long_text.count(phrase.lower()) == 1, f"{q.id}: answer phrase must occur exactly once: {phrase!r}"
        assert (q.kind == "unanswerable") != q.answerable, f"{q.id}: kind and labels disagree"


_validate()

__all__ = [
    "Doc", "LongDoc", "Query", "KINDS",
    "CORE_DOCS", "CORE_QUERIES", "EVAL_DOCS", "LONG_DOCS", "EVAL_QUERIES", "DOC_BY_ID", "ID_BY_TEXT",
    "docs_in", "queries_of", "adversarial", "cjk",
    "SENTENCE_WINDOW_TEXT", "SENTENCE_WINDOW_QUERY", "SENTENCE_WINDOW_ANSWER", "SWARM_TASKS", "POISON_TEXT",
    "synthetic_doc", "synthetic_docs",
]
