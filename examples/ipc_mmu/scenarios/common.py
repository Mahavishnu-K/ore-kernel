"""Agent ids and small helpers shared by the scenarios."""

from pathlib import Path

from mmu_client import get_ore_dir

COORDINATOR = "mmu_coordinator"
RESEARCHER = "mmu_researcher"
WORKER_A = "mmu_worker_a"
WORKER_B = "mmu_worker_b"
REVIEWER = "mmu_reviewer"
ANALYST = "mmu_analyst"
BULK = "mmu_bulk"
ARCHIVIST = "mmu_archivist"
OUTSIDER = "mmu_outsider"
BENCH = "mmu_bench"
CRAG = "mmu_crag"
GUARD = "mmu_guard"
WEB = "mmu_web"
GHOST = "mmu_ghost_never_registered"

ALL_AGENTS = [COORDINATOR, RESEARCHER, WORKER_A, WORKER_B, REVIEWER, ANALYST, BULK, ARCHIVIST, OUTSIDER, BENCH, CRAG, WEB, GUARD]

LLM_MODEL = "qwen2.5:0.5b"


def kernel_memory_dir(client) -> Path:
    """The kernel's memory/ directory. /health reports ore_dir; when that is
    relative (the kernel found ore.toml in its own working directory), resolve
    it the way the Python client does."""
    try:
        ore_dir = Path(client.health_json().get("ore_dir", ""))
    except Exception:
        ore_dir = Path()
    if not ore_dir.is_absolute():
        ore_dir = get_ore_dir()
    return ore_dir / "memory"


def texts(results: list) -> list:
    return [r["text"] for r in results]


def rank_of(results: list, text: str):
    """1-based rank of the result whose text is exactly `text`, or None."""
    for i, r in enumerate(results, 1):
        if r["text"] == text:
            return i
    return None
