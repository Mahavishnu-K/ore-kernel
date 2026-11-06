"""Types shared by every corpus module."""

from dataclasses import dataclass

# Query kinds, in report order.
KINDS = ("exact", "semantic", "near_miss", "multi_doc", "technical", "long", "cjk", "unanswerable")


@dataclass(frozen=True)
class Doc:
    id: str
    category: str  # fact | skill | directive | general
    text: str
    lang: str = "en"


@dataclass(frozen=True)
class LongDoc:
    """A multi-paragraph document that is chunked, not stored whole.
    Paragraphs are separated by blank lines; sentences end with ". "."""

    id: str
    title: str
    category: str
    text: str


@dataclass(frozen=True)
class Query:
    """A labelled question.

    relevant  - ids of documents stored whole (one chunk each); order doesn't matter
    answers   - phrases for chunked documents: a result is relevant if its text or
                context_payload contains one of them (case-insensitive)
    Neither   - the corpus can't answer it (used to calibrate the CRAG gate)
    """

    id: str
    text: str
    kind: str
    relevant: tuple = ()
    answers: tuple = ()
    note: str = ""

    @property
    def answerable(self) -> bool:
        return bool(self.relevant or self.answers)
