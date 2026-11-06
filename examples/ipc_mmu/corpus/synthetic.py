"""Synthetic bulk records for benchmarks and Mode C.

Good for latency (dense search cost doesn't depend on content); not for quality:
they come from ~100 distinct words, so BM25 posting lists are near worst case and
embeddings sit close together. Use the labelled corpus for recall."""

import random

_ADJ = ["amber", "brisk", "cobalt", "dusty", "early", "faint", "gentle", "hollow", "ivory", "jagged",
        "keen", "lunar", "mellow", "narrow", "olive", "pale", "quiet", "rapid", "silver", "tidal"]
_NOUN = ["turbine", "ledger", "sensor", "gateway", "harbor", "kiln", "lattice", "beacon", "conduit", "relay",
         "furnace", "orchard", "quarry", "reactor", "spindle", "tunnel", "vault", "winch", "yard", "zeppelin"]
_PLACE = ["Arden", "Bexley", "Corran", "Dunmore", "Elgin", "Fenwick", "Galloway", "Hawick", "Inverness", "Jarrow"]
_VERB = ["recorded", "measured", "flagged", "logged", "observed", "reported", "detected", "sampled"]
_METRIC = ["throughput", "pressure", "temperature", "vibration", "voltage", "latency", "humidity", "torque"]
_PERIOD = ["the night shift", "the morning audit", "the weekend window", "the quarterly test", "the spring survey"]


def synthetic_doc(i: int, seed: int = 7) -> str:
    """Deterministic, unique, 24-word record. Same (i, seed) -> same text, so
    re-sharing hits the kernel's embedding cache."""
    r = random.Random(seed * 1_000_003 + i)
    return (
        f"Record R{i:06d}: the {r.choice(_ADJ)} {r.choice(_NOUN)} at {r.choice(_PLACE)} station "
        f"{r.choice(_VERB)} {r.choice(_METRIC)} of {r.randint(10, 9999)} units during "
        f"{r.choice(_PERIOD)}, filed under batch B{r.randint(100, 999)} for later review."
    )


def synthetic_docs(n: int, start: int = 0, seed: int = 7) -> list:
    return [synthetic_doc(i, seed) for i in range(start, start + n)]
