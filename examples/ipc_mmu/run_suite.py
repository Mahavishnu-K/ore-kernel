"""
run_suite.py - Cognitive MMU test suite and benchmarks for the Ore kernel.

    python run_suite.py --install-manifests      # once, then restart the kernel
    python run_suite.py                          # all scenarios
    python run_suite.py --only s05,s08 -v        # a subset, with detail
    python run_suite.py --skip-llm               # no HyDE / RAPTOR / Auto-RAG
    python run_suite.py --bench                  # HTTP benchmarks only
    python run_suite.py --only s09 --phase write # then restart, then --phase verify
    python run_suite.py --cleanup                # delete Mode C / archive files

Exit status is 1 if any scenario fails. Reports go to results/run_<id>.{json,md}.
"""

import argparse
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import harness  # noqa: E402
from mmu_client import OreMmuClient, get_ore_dir  # noqa: E402

MANIFESTS = HERE / "manifests"


def parse_args():
    p = argparse.ArgumentParser(description="Ore Cognitive MMU suite")
    p.add_argument("--only", help="comma-separated scenario id prefixes, e.g. s05,s08")
    p.add_argument("--skip-llm", action="store_true", help="skip scenarios that need a model")
    p.add_argument("--phase", choices=["write", "verify"], help="s09 persistence phase")
    p.add_argument("--crag-tool", help="WASM tool for the s11 CRAG fallback")
    p.add_argument("--bench", action="store_true", help="run the HTTP benchmarks instead of the scenarios")
    p.add_argument("--sizes", default="1000,5000,10000", help="benchmark pipe sizes (default 1000,5000,10000)")
    p.add_argument("--concurrency", default="1,4,16", help="benchmark searcher threads (default 1,4,16)")
    p.add_argument("--iterations", type=int, default=200, help="timed iterations per benchmark")
    p.add_argument("--warmup", type=int, default=20, help="untimed warm-up iterations")
    p.add_argument("--install-manifests", action="store_true", help="copy manifests/*.toml into <ore_dir>/manifests")
    p.add_argument("--cleanup", action="store_true", help="delete memory/ files this suite created")
    p.add_argument("--yes", action="store_true", help="don't ask before --cleanup deletes")
    p.add_argument("--json", action="store_true", help="end with the JSON report path instead of the summary")
    p.add_argument("-v", "--verbose", action="store_true")
    return p.parse_args()


def install_manifests() -> int:
    target = get_ore_dir() / "manifests"
    target.mkdir(parents=True, exist_ok=True)
    files = sorted(MANIFESTS.glob("*.toml"))
    for f in files:
        shutil.copy2(f, target / f.name)
        print(f"  installed {f.name}")
    print(f"\n{len(files)} manifests copied to {target}")
    print("Restart the kernel so it registers them (manifests are read at boot).")
    return 0


def cleanup(client, assume_yes: bool) -> int:
    from scenarios.common import kernel_memory_dir

    memory = kernel_memory_dir(client)
    patterns = ["mmu_bulk_*", "mmu_archive_*", "mmu_*.corrupt-*"]
    victims = sorted({p for pat in patterns for p in memory.glob(pat)})
    if not victims:
        print(f"Nothing to clean in {memory}")
        return 0
    for v in victims:
        print(f"  {v.name}")
    if not assume_yes and input(f"Delete these {len(victims)} files from {memory}? [y/N] ").strip().lower() != "y":
        print("Nothing deleted.")
        return 0
    for v in victims:
        v.unlink()
    print(f"Deleted {len(victims)} files. Restart the kernel to drop the pipes from RAM as well.")
    return 0


def preflight(client) -> None:
    from scenarios.common import ALL_AGENTS

    registered = {a["app_id"] for a in client.agents_json()}
    missing = [a for a in ALL_AGENTS if a not in registered]
    if missing:
        print("These suite agents are not registered with the kernel:")
        for m in missing:
            print(f"  - {m}")
        print("\nRun `python run_suite.py --install-manifests`, then restart the kernel.")
        sys.exit(2)


def main() -> int:
    args = parse_args()
    if args.install_manifests:
        return install_manifests()

    client = OreMmuClient()
    if args.cleanup:
        return cleanup(client, args.yes)

    preflight(client)
    run_id = harness.new_run_id()
    ctx = harness.Ctx(client, args, run_id)
    env = harness.environment(client)
    print(f"Ore Cognitive MMU suite - run {run_id}")
    kernel = env.get("kernel", {})
    print(f"Kernel v{kernel.get('version', '?')} on {kernel.get('device', '?')} ({kernel.get('engine', '?')})\n")

    results, bench = [], []
    if args.bench:
        import bench_mmu

        print("Benchmarks (HTTP, end to end):")
        bench = bench_mmu.run_benchmarks(ctx)
    else:
        import scenarios  # noqa: F401  (registers every scenario)
        from scenarios.common import LLM_MODEL

        if args.skip_llm:
            ctx.llm_skip_reason = "--skip-llm"
        elif not client.has_model(LLM_MODEL):
            ctx.llm_skip_reason = f"model {LLM_MODEL} is not installed (ore pull {LLM_MODEL})"

        selected = harness.SCENARIOS
        if args.only:
            prefixes = [p.strip() for p in args.only.split(",") if p.strip()]
            selected = [s for s in selected if any(s.id.startswith(p) for p in prefixes)]
        if not selected:
            print(f"No scenario matches --only {args.only}")
            return 2
        results = harness.run_scenarios(ctx, selected)

    json_path, md_path = harness.write_report(run_id, args, env, results, bench, ctx.sections)
    if args.json:
        print(json_path)
    else:
        failed = sum(r.status == "FAIL" for r in results)
        passed = sum(r.status == "PASS" for r in results)
        skipped = sum(r.status == "SKIP" for r in results)
        gaps = sum(r.status == "XFAIL" for r in results)
        if results:
            print(f"\n{passed} passed, {failed} failed, {gaps} known gaps, {skipped} skipped")
        print(f"Report: {md_path}")
    return 1 if any(r.status == "FAIL" for r in results) else 0


if __name__ == "__main__":
    sys.exit(main())
