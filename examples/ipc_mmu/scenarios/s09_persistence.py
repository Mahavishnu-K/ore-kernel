"""s09 - Crash-safe persistence across a kernel restart (two phases).

  python run_suite.py --only s09 --phase write
  (restart the kernel)
  python run_suite.py --only s09 --phase verify

The write phase stores uniquely tagged chunks in a persistent pipe and saves
what it wrote to results/s09_state.json. The verify phase, after the
restart, checks every chunk is still there exactly once.
"""

import json
import time

from harness import RESULTS_DIR, Skip, scenario

from .common import ARCHIVIST, kernel_memory_dir, rank_of

PIPE = "mmu_archive_s09"  # fixed name: it has to outlive the run that wrote it
STATE = RESULTS_DIR / "s09_state.json"


@scenario("s09_persistence", "Persistence: chunks survive a kernel restart")
def run(ctx):
    phase = getattr(ctx.args, "phase", None)
    if phase is None:
        raise Skip("two-phase test: run with --phase write, restart the kernel, then --phase verify")

    c = ctx.client
    if phase == "write":
        # Clean up any leftover archive from previous runs so duplicates don't compete
        if STATE.exists():
            STATE.unlink()
        mem_dir = kernel_memory_dir(c)
        for old_file in [mem_dir / f"{PIPE}.pipe", mem_dir / f"{PIPE}.sq8pipe"]:
            if old_file.exists():
                try:
                    old_file.unlink()
                except Exception:
                    pass
        chunks = [f"[{ctx.run_id}] archived ledger entry {i}: settlement batch S{i:04d} closed." for i in range(5)]
        for text in chunks:
            c.share(ARCHIVIST, PIPE, text, category="fact", chunk_strategy="exact_match")
        time.sleep(1.0)  # persistence runs on a background task after each share
        RESULTS_DIR.mkdir(exist_ok=True)
        STATE.write_text(json.dumps({"run_id": ctx.run_id, "chunks": chunks}), encoding="utf-8")
        for text in chunks:
            ctx.expect(rank_of(c.search(ARCHIVIST, PIPE, text, top_k=3), text) == 1, f"write phase can't read back: {text}")
        ctx.metric("chunks_written", len(chunks))
        print(f"      wrote {len(chunks)} chunks to '{PIPE}'. Restart the kernel, then run --phase verify")
        return

    if not STATE.exists():
        raise Skip("no s09 state found: run --phase write first")
    state = json.loads(STATE.read_text(encoding="utf-8"))
    for text in state["chunks"]:
        results = c.search(ARCHIVIST, PIPE, text, top_k=5)
        ctx.expect(rank_of(results, text) == 1, f"lost after restart: {text}")
        ctx.expect([r["text"] for r in results].count(text) == 1, f"duplicated after restart: {text}")
    ctx.metric("chunks_verified", len(state["chunks"]))
    ctx.metric("written_by_run", state["run_id"])
