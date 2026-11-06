"""Poisoned and sensitive text for the retrieved-context firewall.

The cases live in adversarial.json so the offline Rust test
(ore-core/tests/firewall_corpus.rs) reads exactly the same data.
"""

import json
from dataclasses import dataclass, field
from pathlib import Path

_DATA = json.loads((Path(__file__).with_name("adversarial.json")).read_text(encoding="utf-8"))


@dataclass(frozen=True)
class Case:
    id: str
    kind: str  # injection | pii | benign
    expect: str  # drop | redact | pass | log_only | known_evasion | known_false_positive
    text: str
    secrets: tuple = field(default_factory=tuple)
    note: str = ""


CASES = [
    Case(c["id"], c["kind"], c["expect"], c["text"], tuple(c.get("secrets", ())), c.get("note", ""))
    for c in _DATA["cases"]
]
EXPECTATIONS = _DATA["expectations"]


def cases(expect: str) -> list:
    return [c for c in CASES if c.expect == expect]
