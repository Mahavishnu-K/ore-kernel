"""s14 - CRAG confidence calibration: where should the threshold be?

For every labelled query, records the top score each gate design would see:
  dense         top cosine similarity
  hybrid_fused  top normalized RRF score
  kernel_gate   max(hybrid_fused, dense), what CRAG compares today
and compares answerable against unanswerable queries. The report shows the
distributions, a histogram, the AUC, and the threshold that best separates them.

Normalized RRF ranks rather than measures: the top result of a one-retriever
match always scores 1.0, and it never drops below 0.5. So kernel_gate barely
falls back, whatever the threshold. That is recorded as a known gap (XFAIL)
until the gate uses a similarity score.
"""

import corpus
import evaluation
import metrics
from harness import scenario

from .common import RESEARCHER

DEFAULT_THRESHOLD = 0.70  # [rag].confidence_threshold default


@scenario("s14_crag_calibration", "CRAG gate calibration: answerable vs unanswerable scores")
def run(ctx):
    run = evaluation.cached_run(ctx, RESEARCHER)
    signals = evaluation.confidence_signals(run)
    answerable = [q.id for q in corpus.EVAL_QUERIES if q.answerable]
    unanswerable = [q.id for q in corpus.EVAL_QUERIES if not q.answerable]
    near = [q.id for q in corpus.EVAL_QUERIES if q.note == "near_domain"]

    cals = []
    for name, values in signals.items():
        a = [values[i] for i in answerable]
        u = [values[i] for i in unanswerable]
        cals.append(metrics.calibrate(name, a, u, DEFAULT_THRESHOLD))
    dense = next(c for c in cals if c.signal == "dense")
    gate = next(c for c in cals if c.signal == "kernel_gate")

    near_scores = [signals["dense"][i] for i in near]
    near_cal = metrics.calibrate("dense (near-domain negatives only)", [signals["dense"][i] for i in answerable], near_scores)

    body = [
        f"{len(answerable)} answerable vs {len(unanswerable)} unanswerable queries "
        f"({len(near)} of them near-domain: plausible questions that share tokens with stored text).",
        "",
        metrics.calibration_table(cals + [near_cal], DEFAULT_THRESHOLD),
        "",
        "Catch = unanswerable queries that fall back (good). False fallback = answerable queries that fall back (wasted tool runs).",
        "",
        "Score distributions (dense):",
        "",
        f"- answerable: {dense.answerable}",
        f"- unanswerable: {dense.unanswerable}",
        "",
        "Histogram, dense cosine (A = answerable, U = unanswerable):",
        "",
        "```",
        metrics.histogram([signals["dense"][i] for i in answerable], [signals["dense"][i] for i in unanswerable]),
        "```",
        "",
        "Histogram, kernel gate max(fused, dense):",
        "",
        "```",
        metrics.histogram([signals["kernel_gate"][i] for i in answerable], [signals["kernel_gate"][i] for i in unanswerable]),
        "```",
    ]
    ctx.section("CRAG threshold calibration", "\n".join(body))

    ctx.metric("dense_auc", round(dense.auc, 3))
    ctx.metric("dense_suggested_threshold", dense.best_t)
    ctx.metric("dense_catch@suggested", round(dense.best_catch, 3))
    ctx.metric("dense_false_fallback@suggested", round(dense.best_false, 3))
    ctx.metric("kernel_gate_auc", round(gate.auc, 3))
    ctx.metric("kernel_gate_catch@0.70", round(gate.at_default[0], 3))
    ctx.log(f"dense: AUC {dense.auc:.3f}, suggested threshold {dense.best_t:.2f} "
            f"(catch {dense.best_catch:.0%}, false fallback {dense.best_false:.0%})")
    ctx.log(f"kernel gate at {DEFAULT_THRESHOLD}: catch {gate.at_default[0]:.0%}, false fallback {gate.at_default[1]:.0%}")

    ctx.expect(dense.auc >= 0.75, f"dense similarity barely separates answerable from unanswerable (AUC {dense.auc:.3f})")
    ctx.known_gap(
        gate.at_default[0] >= 0.5,
        f"kernel CRAG gate catches {gate.at_default[0]:.0%} of unanswerable queries at {DEFAULT_THRESHOLD} "
        f"(dense alone would catch {dense.at_default[0]:.0%}); normalized RRF keeps the top score at 0.5-1.0",
    )
