"""
harness.py - Minimal scenario runner and benchmark statistics.

No pytest dependency: scenarios register with @scenario, run in order, and
end as PASS, FAIL, or SKIP. Results and benchmark tables are written to
results/run_<id>.json and results/run_<id>.md.
"""

import json
import os
import platform
import secrets
import statistics
import sys
import time
import traceback
from dataclasses import asdict, dataclass, field
from pathlib import Path

import requests

RESULTS_DIR = Path(__file__).resolve().parent / "results"

# ─── Terminal colours ────────────────────────────────────────────

if os.name == "nt":
    os.system("")  # enables ANSI escape processing in the Windows console

_TTY = sys.stdout.isatty()


def paint(text: str, code: str) -> str:
    return f"\033[{code}m{text}\033[0m" if _TTY else text


STATUS_COLOUR = {"PASS": "32", "FAIL": "31", "SKIP": "33", "XFAIL": "35"}
STATUSES = ("PASS", "FAIL", "XFAIL", "SKIP")

# ─── Scenario registry ───────────────────────────────────────────


class Skip(Exception):
    """Raise inside a scenario to end it as SKIP with a reason."""


@dataclass
class Scenario:
    id: str
    title: str
    needs_llm: bool
    fn: object


SCENARIOS: list = []


def scenario(id: str, title: str, needs_llm: bool = False):
    def register(fn):
        SCENARIOS.append(Scenario(id, title, needs_llm, fn))
        return fn

    return register


@dataclass
class Result:
    id: str
    title: str
    status: str
    duration_ms: float
    detail: str = ""
    metrics: dict = field(default_factory=dict)


class Ctx:
    """Shared state handed to every scenario."""

    def __init__(self, client, args, run_id: str):
        self.client = client
        self.args = args
        self.run_id = run_id
        self.metrics: dict = {}
        self.verbose = getattr(args, "verbose", False)
        self.llm_skip_reason: str = None  # set by the runner when no model is available
        self.shared: dict = {}  # state reused across scenarios in one run (e.g. the eval index)
        self.sections: list = []  # (title, markdown) blocks appended to the report
        self.gaps: list = []  # known gaps hit by the current scenario

    def pipe(self, prefix: str, name: str) -> str:
        """Run-scoped pipe name, e.g. pipe("mmu_kb", "s05") -> mmu_kb_<run>_s05."""
        return f"{prefix}_{self.run_id}_{name}"

    def expect(self, cond: bool, message: str):
        if not cond:
            raise AssertionError(message)

    def known_gap(self, cond: bool, message: str):
        """Like expect(), for a documented limitation: the scenario keeps running and
        ends as XFAIL instead of FAIL. Once the gap is fixed, the check passes."""
        if not cond:
            self.gaps.append(message)

    def metric(self, key: str, value):
        self.metrics[key] = value

    def section(self, title: str, markdown: str):
        self.sections.append((title, markdown))

    def log(self, message: str):
        if self.verbose:
            print(paint(f"      {message}", "2"))


def new_run_id() -> str:
    return time.strftime("%Y%m%d_%H%M%S") + "_" + secrets.token_hex(2)


def kernel_alive(client) -> bool:
    try:
        return requests.get(f"{client.base_url}/health", headers=client.headers, timeout=5).status_code == 200
    except requests.RequestException:
        return False


def run_scenarios(ctx: Ctx, selected: list) -> list:
    results = []
    for index, sc in enumerate(selected):
        print(f"  {sc.id:<20} {sc.title} ...", end="", flush=True)
        ctx.metrics = {}
        ctx.gaps = []
        start = time.perf_counter()
        try:
            if sc.needs_llm and ctx.llm_skip_reason:
                raise Skip(ctx.llm_skip_reason)
            sc.fn(ctx)
            status, detail = ("XFAIL", "known gap: " + "; ".join(ctx.gaps)) if ctx.gaps else ("PASS", "")
        except Skip as e:
            status, detail = "SKIP", str(e)
        except AssertionError as e:
            status, detail = "FAIL", str(e) or "assertion failed"
        except requests.RequestException as e:
            # Lost the kernel (crashed, restarted, or hung past the timeout).
            status, detail = "FAIL", f"kernel request failed: {type(e).__name__}: {e}"
        except Exception as e:  # a crash is a failure, with the trace for debugging
            status = "FAIL"
            detail = f"{type(e).__name__}: {e}"
            if ctx.verbose:
                traceback.print_exc()
        elapsed = (time.perf_counter() - start) * 1000
        print(f"\r  {sc.id:<20} {sc.title:<58} {paint(status, STATUS_COLOUR[status])} {elapsed:>9.0f} ms")
        if detail:
            print(paint(f"      {detail}", STATUS_COLOUR[status]))
        results.append(Result(sc.id, sc.title, status, round(elapsed, 1), detail, dict(ctx.metrics)))

        if status == "FAIL" and detail.startswith("kernel request failed") and not kernel_alive(ctx.client):
            skipped = selected[index + 1 :]
            print(paint(f"\n  The kernel at {ctx.client.base_url} is not responding. Check its terminal for a panic "
                        f"or a hang, restart it, then re-run. Skipping {len(skipped)} remaining scenarios.", "31"))
            results += [Result(s.id, s.title, "SKIP", 0.0, "kernel not responding") for s in skipped]
            break
    return results


# ─── Statistics ──────────────────────────────────────────────────


def percentile(values: list, p: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    k = (len(s) - 1) * p / 100
    lo, hi = int(k), min(int(k) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def summarize(samples_ms: list, wall_s: float = None) -> dict:
    """p50/p95/p99/mean in ms; ops/s from wall time when given (concurrent runs)."""
    if not samples_ms:
        return {"n": 0}
    total_s = wall_s if wall_s is not None else sum(samples_ms) / 1000
    return {
        "n": len(samples_ms),
        "p50_ms": round(percentile(samples_ms, 50), 3),
        "p95_ms": round(percentile(samples_ms, 95), 3),
        "p99_ms": round(percentile(samples_ms, 99), 3),
        "mean_ms": round(statistics.fmean(samples_ms), 3),
        "min_ms": round(min(samples_ms), 3),
        "max_ms": round(max(samples_ms), 3),
        "ops_s": round(len(samples_ms) / total_s, 1) if total_s > 0 else 0.0,
    }


def timed(fn) -> tuple:
    """Runs fn() once. Returns (elapsed_ms, result)."""
    start = time.perf_counter()
    out = fn()
    return (time.perf_counter() - start) * 1000, out


def sample(fn, iterations: int, warmup: int = 0) -> list:
    """Calls fn(i) `warmup` times untimed, then `iterations` times timed (ms)."""
    for i in range(warmup):
        fn(i)
    out = []
    for i in range(iterations):
        start = time.perf_counter()
        fn(i)
        out.append((time.perf_counter() - start) * 1000)
    return out


# ─── Reports ─────────────────────────────────────────────────────


def environment(client) -> dict:
    env = {"python": platform.python_version(), "platform": platform.platform()}
    try:
        env["kernel"] = client.health_json()
        env["models"] = [m["name"] for m in client.models_json()]
    except Exception as e:
        env["kernel_error"] = str(e)
    return env


def write_report(run_id: str, args, env: dict, results: list, bench: list, sections: list = ()) -> tuple:
    RESULTS_DIR.mkdir(exist_ok=True)
    json_path = RESULTS_DIR / f"run_{run_id}.json"
    md_path = RESULTS_DIR / f"run_{run_id}.md"

    doc = {
        "run_id": run_id,
        "args": {k: v for k, v in vars(args).items() if not callable(v)},
        "environment": env,
        "scenarios": [asdict(r) for r in results],
        "benchmarks": bench,
        "sections": [{"title": t, "markdown": m} for t, m in sections],
    }
    json_path.write_text(json.dumps(doc, indent=2), encoding="utf-8")

    kernel = env.get("kernel", {})
    lines = [
        f"# Ore Cognitive MMU run `{run_id}`",
        "",
        f"- Kernel: v{kernel.get('version', '?')}, engine `{kernel.get('engine', '?')}`, "
        f"device `{kernel.get('device', '?')}`",
        f"- Models on disk: {', '.join(env.get('models', [])) or 'unknown'}",
        f"- Host: {env.get('platform')}, Python {env.get('python')}",
        "",
    ]
    if results:
        counts = {s: sum(r.status == s for r in results) for s in STATUSES}
        lines += [
            f"## Scenarios ({counts['PASS']} pass, {counts['FAIL']} fail, "
            f"{counts['XFAIL']} known gap, {counts['SKIP']} skip)",
            "",
            "| ID | Scenario | Status | Time (ms) | Detail |",
            "|---|---|---|---:|---|",
        ]
        for r in results:
            detail = r.detail.replace("|", "\\|").replace("\n", " ")
            lines.append(f"| {r.id} | {r.title} | {r.status} | {r.duration_ms:.0f} | {detail} |")
        lines.append("")
    if bench:
        lines += [
            "## Benchmarks",
            "",
            "End-to-end over HTTP on localhost: every number includes request parsing, the",
            "manifest permission check, and JSON encoding. Search queries are embedded once",
            "during warm-up and then served from the kernel's embedding cache, so search",
            "timings measure retrieval, not the embedder. For in-process numbers without HTTP,",
            "run `cargo bench -p ore-core --bench cognitive_mmu`.",
            "",
            "| Benchmark | Params | n | p50 ms | p95 ms | p99 ms | mean ms | ops/s | Notes |",
            "|---|---|---:|---:|---:|---:|---:|---:|---|",
        ]
        for b in bench:
            s = b.get("stats", {})
            lines.append(
                f"| {b['name']} | {b.get('params', '')} | {s.get('n', '')} | {s.get('p50_ms', '')} | "
                f"{s.get('p95_ms', '')} | {s.get('p99_ms', '')} | {s.get('mean_ms', '')} | "
                f"{s.get('ops_s', '')} | {b.get('notes', '')} |"
            )
        lines.append("")
    for title, markdown in sections:
        lines += [f"## {title}", "", markdown, ""]
    md_path.write_text("\n".join(lines), encoding="utf-8")
    return json_path, md_path
