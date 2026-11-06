"""
metrics.py - Retrieval metrics and CRAG threshold calibration.

Retrieval (answerable queries, binary relevance):
  recall@k   fraction of a query's relevant items found in the top k
             (a query answered by phrase counts as one item)
  hit@k      1 if anything relevant is in the top k
  MRR        mean of 1 / rank of the first relevant result (0 if none in the cutoff)
  nDCG@10    rank-discounted gain, normalized by the ideal ranking

Calibration: a CRAG gate falls back when confidence < threshold. Treating
"unanswerable" as the positive class, a threshold t gives
  catch rate        share of unanswerable queries below t (correct fallbacks)
  false fallback    share of answerable queries below t (wasted tool runs)
The suggested threshold maximizes catch rate - false fallback (Youden's J).
AUC is the probability that a random answerable query scores higher than a
random unanswerable one: 1.0 separates perfectly, 0.5 is a coin flip.
"""

import math
import statistics
from dataclasses import dataclass, field

import corpus

CUTOFFS = (1, 3, 5, 10)


# ─── Relevance ───────────────────────────────────────────────────


def is_relevant(query, result: dict) -> bool:
    if query.answers:
        haystack = f"{result.get('text', '')}\n{result.get('context_payload', '')}".lower()
        return any(a.lower() in haystack for a in query.answers)
    return corpus.ID_BY_TEXT.get(result.get("text")) in query.relevant


def relevance_vector(query, results: list) -> list:
    """1/0 per rank. For multi-document queries each relevant doc counts once."""
    seen, vec = set(), []
    for r in results:
        if query.answers:
            vec.append(1 if is_relevant(query, r) and not seen else 0)
            if vec[-1]:
                seen.add("answer")
        else:
            doc_id = corpus.ID_BY_TEXT.get(r.get("text"))
            hit = doc_id in query.relevant and doc_id not in seen
            if hit:
                seen.add(doc_id)
            vec.append(1 if hit else 0)
    return vec


@dataclass
class QueryScore:
    query_id: str
    kind: str
    recall: dict
    hit: dict
    rr: float
    ndcg10: float
    first_rank: int = None
    top_text: str = ""


def score_query(query, results: list) -> QueryScore:
    vec = relevance_vector(query, results)
    total = 1 if query.answers else len(query.relevant)
    recall = {k: sum(vec[:k]) / total for k in CUTOFFS}
    hit = {k: 1.0 if any(vec[:k]) else 0.0 for k in CUTOFFS}
    first = next((i + 1 for i, v in enumerate(vec) if v), None)
    dcg = sum(v / math.log2(i + 2) for i, v in enumerate(vec[:10]))
    idcg = sum(1 / math.log2(i + 2) for i in range(min(total, 10)))
    return QueryScore(
        query.id,
        query.kind,
        recall,
        hit,
        1 / first if first else 0.0,
        dcg / idcg if idcg else 0.0,
        first,
        results[0]["text"][:80] if results else "",
    )


@dataclass
class Summary:
    label: str
    n: int
    recall: dict = field(default_factory=dict)
    hit: dict = field(default_factory=dict)
    mrr: float = 0.0
    ndcg10: float = 0.0


def summarize(label: str, scores: list) -> Summary:
    if not scores:
        return Summary(label, 0)
    return Summary(
        label,
        len(scores),
        {k: statistics.fmean(s.recall[k] for s in scores) for k in CUTOFFS},
        {k: statistics.fmean(s.hit[k] for s in scores) for k in CUTOFFS},
        statistics.fmean(s.rr for s in scores),
        statistics.fmean(s.ndcg10 for s in scores),
    )


def by_kind(scores: list) -> list:
    """One Summary per query kind (report order), then the overall row."""
    rows = [summarize(k, [s for s in scores if s.kind == k]) for k in corpus.KINDS]
    rows = [r for r in rows if r.n]
    rows.append(summarize("all answerable", scores))
    return rows


def retrieval_table(modes: dict) -> str:
    """Markdown table; `modes` maps a mode label to a list of Summary rows."""
    lines = [
        "| Mode | Query kind | n | R@1 | R@3 | R@5 | R@10 | MRR | nDCG@10 |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for mode, rows in modes.items():
        for r in rows:
            lines.append(
                f"| {mode} | {r.label} | {r.n} | {r.recall[1]:.2f} | {r.recall[3]:.2f} | {r.recall[5]:.2f} | "
                f"{r.recall[10]:.2f} | {r.mrr:.3f} | {r.ndcg10:.3f} |"
            )
    return "\n".join(lines)


def misses_table(scores: list, queries: dict, limit: int = 15) -> str:
    """The worst answerable queries, so regressions are easy to inspect."""
    worst = sorted(scores, key=lambda s: (s.rr, s.recall[10]))[:limit]
    lines = ["| Query | Kind | First relevant rank | Top result |", "|---|---|---:|---|"]
    for s in worst:
        text = queries[s.query_id].text.replace("|", "\\|")
        top = s.top_text.replace("|", "\\|").replace("\n", " ")
        lines.append(f"| `{s.query_id}` {text} | {s.kind} | {s.first_rank or '-'} | {top} |")
    return "\n".join(lines)


# ─── Calibration ─────────────────────────────────────────────────


def distribution(values: list) -> dict:
    if not values:
        return {}
    s = sorted(values)

    def pct(p):
        k = (len(s) - 1) * p
        lo, hi = int(k), min(int(k) + 1, len(s) - 1)
        return s[lo] + (s[hi] - s[lo]) * (k - lo)

    return {
        "n": len(s),
        "min": round(s[0], 3),
        "p10": round(pct(0.10), 3),
        "p25": round(pct(0.25), 3),
        "p50": round(pct(0.50), 3),
        "p75": round(pct(0.75), 3),
        "p90": round(pct(0.90), 3),
        "max": round(s[-1], 3),
    }


def auc(answerable: list, unanswerable: list) -> float:
    """P(score of a random answerable > score of a random unanswerable); ties count half."""
    if not answerable or not unanswerable:
        return float("nan")
    wins = sum(1.0 if a > u else 0.5 if a == u else 0.0 for a in answerable for u in unanswerable)
    return wins / (len(answerable) * len(unanswerable))


def gate_rates(answerable: list, unanswerable: list, threshold: float) -> tuple:
    """(catch rate, false fallback rate) for a gate that falls back below `threshold`."""
    catch = sum(v < threshold for v in unanswerable) / len(unanswerable) if unanswerable else 0.0
    false = sum(v < threshold for v in answerable) / len(answerable) if answerable else 0.0
    return catch, false


def best_threshold(answerable: list, unanswerable: list) -> tuple:
    """Threshold in [0, 1] (0.01 steps) maximizing catch - false fallback. Ties go to the lower t."""
    best = (0.0, -1.0, 0.0, 0.0)
    for i in range(101):
        t = i / 100
        catch, false = gate_rates(answerable, unanswerable, t)
        j = catch - false
        if j > best[1] + 1e-9:
            best = (t, j, catch, false)
    return best


@dataclass
class Calibration:
    signal: str
    answerable: dict
    unanswerable: dict
    auc: float
    best_t: float
    best_j: float
    best_catch: float
    best_false: float
    at_default: tuple


def calibrate(signal: str, answerable: list, unanswerable: list, default_t: float = 0.70) -> Calibration:
    t, j, catch, false = best_threshold(answerable, unanswerable)
    return Calibration(
        signal,
        distribution(answerable),
        distribution(unanswerable),
        auc(answerable, unanswerable),
        t,
        j,
        catch,
        false,
        gate_rates(answerable, unanswerable, default_t),
    )


def histogram(answerable: list, unanswerable: list, bins: int = 10, width: int = 30) -> str:
    """Text histogram of both score sets side by side (A = answerable, U = unanswerable)."""
    counts_a, counts_u = [0] * bins, [0] * bins
    for v in answerable:
        counts_a[min(int(v * bins), bins - 1)] += 1
    for v in unanswerable:
        counts_u[min(int(v * bins), bins - 1)] += 1
    peak = max(counts_a + counts_u + [1])
    lines = []
    for b in range(bins):
        lo, hi = b / bins, (b + 1) / bins
        bar_a = "A" * round(counts_a[b] / peak * width)
        bar_u = "U" * round(counts_u[b] / peak * width)
        lines.append(f"{lo:.1f}-{hi:.1f} | {bar_a:<{width}} | {bar_u:<{width}} | {counts_a[b]:>3} / {counts_u[b]:>3}")
    return "\n".join(lines)


def calibration_table(rows: list, default_t: float) -> str:
    lines = [
        f"| Confidence signal | AUC | Answerable p50 | Unanswerable p50 | Suggested threshold | Catch @ suggested | "
        f"False fallback @ suggested | Catch @ {default_t:.2f} | False fallback @ {default_t:.2f} |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for c in rows:
        lines.append(
            f"| {c.signal} | {c.auc:.3f} | {c.answerable.get('p50', '-')} | {c.unanswerable.get('p50', '-')} | "
            f"{c.best_t:.2f} | {c.best_catch:.0%} | {c.best_false:.0%} | {c.at_default[0]:.0%} | {c.at_default[1]:.0%} |"
        )
    return "\n".join(lines)
