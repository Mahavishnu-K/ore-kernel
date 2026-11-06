"""s10 - Storage modes: automatic Mode A -> Mode C migration.

mmu_bulk's manifest sets max_hot_chunks = 256. Writing past that converts
the pipe to Mode C: int8 (SQ8) vectors in RAM plus full-precision vectors in
a memory-mapped <pipe>.f32vecs file. Search must keep working across the
switch, including exact-ID matching through BM25.
"""

import random

import corpus
from harness import scenario

from .common import BULK, kernel_memory_dir, rank_of

HOT_LIMIT = 256
TOTAL = 600


@scenario("s10_storage_modes", "Mode A -> Mode C (SQ8 + mmap) migration keeps recall")
def run(ctx):
    c = ctx.client
    pipe = ctx.pipe("mmu_bulk", "s10")
    docs = corpus.synthetic_docs(TOTAL)

    c.share_docs(BULK, pipe, docs[: HOT_LIMIT - 6])
    sample_a = random.Random(1).sample(docs[: HOT_LIMIT - 6], 10)
    hits_a = sum(rank_of(c.search(BULK, pipe, d, top_k=3), d) == 1 for d in sample_a)

    c.share_docs(BULK, pipe, docs[HOT_LIMIT - 6 :])
    sample_c = random.Random(2).sample(docs, 20)
    hits_c = sum(rank_of(c.search(BULK, pipe, d, top_k=3), d) == 1 for d in sample_c)

    # Exact record IDs go through BM25, which Mode C keeps.
    ids = random.Random(3).sample(range(TOTAL), 10)
    id_hits = sum(
        rank_of(c.search(BULK, pipe, f"R{i:06d}", top_k=3, enable_hybrid=True), docs[i]) is not None for i in ids
    )

    ctx.metric("recall@1_before_migration", hits_a / len(sample_a))
    ctx.metric("recall@1_after_migration", hits_c / len(sample_c))
    ctx.metric("id_recall@3_after_migration", id_hits / len(ids))

    ctx.expect(hits_a == len(sample_a), f"Mode A: {hits_a}/{len(sample_a)} stored texts ranked first")
    ctx.expect(hits_c >= len(sample_c) - 1, f"Mode C: only {hits_c}/{len(sample_c)} stored texts ranked first")
    ctx.expect(id_hits >= len(ids) - 1, f"Mode C hybrid found only {id_hits}/{len(ids)} record IDs")

    # The mmap file is visible only when this script shares a disk with the kernel.
    memory_dir = kernel_memory_dir(c)
    if memory_dir.is_dir():
        ctx.expect((memory_dir / f"{pipe}.f32vecs").exists(), f"no {pipe}.f32vecs in {memory_dir}: migration didn't happen")
        ctx.metric("f32vecs_bytes", (memory_dir / f"{pipe}.f32vecs").stat().st_size)
