"""Importing this package registers every scenario with harness.SCENARIOS, in order."""

from . import (  # noqa: F401
    s01_share_search,
    s02_hybrid_bm25,
    s03_memory_tiers,
    s04_sentence_window,
    s05_cow_branches,
    s06_permissions,
    s07_message_bus,
    s08_swarm,
    s09_persistence,
    s10_storage_modes,
    s11_llm_rag,
    s12_web_search,
    s13_retrieval_eval,
    s14_crag_calibration,
    s15_chunking,
    s16_firewall_rag,
)
