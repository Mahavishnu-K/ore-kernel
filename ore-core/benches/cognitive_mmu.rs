//! In-process Cognitive MMU benchmarks: the search core without HTTP or the embedder.
//!
//! Run with `cargo bench -p ore-core --bench cognitive_mmu` (add `-- --quick` for a
//! fast pass). Reports land in `target/criterion/`.
//!
//! Vectors are deterministic pseudo-random unit vectors, so results are comparable
//! across runs. `ORE_DIR` points at a temp directory for the whole run: Mode C
//! `.f32vecs` files and page-in lookups never touch the real `memory/` directory.

use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group};
use ore_common::protocol::{MemoryCategory, VectorMode};
use ore_core::ipc::{Bm25Index, SemanticBus, Sq8Vector, chunk_text};

const DIMS: [usize; 2] = [384, 768];
const SIZES: [usize; 3] = [1_000, 5_000, 10_000];
const HOT_LIMIT: usize = 20_000; // keeps every Mode A pipe below the A -> C migration

const ADJ: [&str; 10] = [
    "amber", "brisk", "cobalt", "dusty", "faint", "hollow", "lunar", "olive", "rapid", "tidal",
];
const NOUN: [&str; 10] = [
    "turbine", "ledger", "sensor", "gateway", "harbor", "beacon", "relay", "reactor", "tunnel",
    "vault",
];
const METRIC: [&str; 6] = [
    "throughput",
    "pressure",
    "temperature",
    "vibration",
    "voltage",
    "latency",
];

/// xorshift64*: tiny, deterministic, no extra dependency.
fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    move || {
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        s.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn unit_vector(seed: u64, dim: usize) -> Vec<f32> {
    let mut next = rng(seed);
    let mut v: Vec<f32> = (0..dim)
        .map(|_| (next() >> 40) as f32 / (1u64 << 24) as f32 - 0.5)
        .collect();
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= norm);
    v
}

fn record_text(i: usize) -> String {
    format!(
        "Record R{i:06} the {} {} reported {} of {} units during the night shift",
        ADJ[i % ADJ.len()],
        NOUN[(i / 10) % NOUN.len()],
        METRIC[(i / 100) % METRIC.len()],
        (i * 7919) % 10_000
    )
}

/// A pipe of `n` chunks; every `skill_every`-th chunk is tagged Skill (0 = none).
fn filled_bus(
    pipe: &str,
    n: usize,
    dim: usize,
    mode: VectorMode,
    skill_every: usize,
) -> SemanticBus {
    let bus = SemanticBus::new(24, 24);
    for i in 0..n {
        let category = if skill_every > 0 && i % skill_every == 0 {
            MemoryCategory::Skill
        } else {
            MemoryCategory::Fact
        };
        let text = record_text(i);
        bus.write_chunk(
            pipe,
            text.clone(),
            text,
            unit_vector(i as u64, dim),
            "bench",
            false,
            category,
            mode,
            HOT_LIMIT,
        );
    }
    bus
}

fn dot_product(c: &mut Criterion) {
    let mut g = c.benchmark_group("dot_product");
    for dim in DIMS {
        let (a, b) = (unit_vector(1, dim), unit_vector(2, dim));
        let (qa, qb) = (Sq8Vector::quantize(&a), Sq8Vector::quantize(&b));
        g.throughput(Throughput::Elements(dim as u64));
        g.bench_with_input(BenchmarkId::new("f32", dim), &dim, |bch, _| {
            bch.iter(|| SemanticBus::dot_product(black_box(&a), black_box(&b)))
        });
        g.bench_with_input(BenchmarkId::new("sq8_int8", dim), &dim, |bch, _| {
            bch.iter(|| Sq8Vector::dot_product(black_box(&qa.data), black_box(&qb.data)))
        });
    }
    g.finish();
}

fn search_pipe(c: &mut Criterion) {
    let mut g = c.benchmark_group("search_pipe");
    g.sample_size(30);
    for dim in DIMS {
        for n in SIZES {
            let pipe = format!("bench_search_{dim}_{n}");
            let bus = filled_bus(&pipe, n, dim, VectorMode::HotSimd, 0);
            let query = unit_vector(999_999, dim);
            let text = "R000123 turbine pressure night shift";
            g.throughput(Throughput::Elements(n as u64));
            for (label, hybrid) in [("dense", false), ("hybrid", true)] {
                g.bench_with_input(
                    BenchmarkId::new(format!("{label}/{dim}d"), n),
                    &n,
                    |bch, _| {
                        bch.iter(|| {
                            bus.search_pipe(&pipe, &query, text, 5, None, None, false, hybrid)
                        })
                    },
                );
            }
        }
    }
    g.finish();
}

fn search_filtered(c: &mut Criterion) {
    let mut g = c.benchmark_group("search_filtered");
    g.sample_size(30);
    let (n, dim) = (10_000, 384);
    let pipe = "bench_filtered";
    let bus = filled_bus(pipe, n, dim, VectorMode::HotSimd, 4);
    let query = unit_vector(999_999, dim);
    g.throughput(Throughput::Elements(n as u64));
    for (label, filter) in [("all", None), ("skill_25pct", Some(MemoryCategory::Skill))] {
        g.bench_function(label, |bch| {
            bch.iter(|| {
                bus.search_pipe(
                    pipe,
                    &query,
                    "turbine pressure",
                    5,
                    None,
                    filter,
                    false,
                    true,
                )
            })
        });
    }
    g.finish();
}

fn bm25(c: &mut Criterion) {
    let mut g = c.benchmark_group("bm25");
    let n = 10_000;
    let docs: Vec<String> = (0..n).map(record_text).collect();
    g.throughput(Throughput::Elements(n as u64));
    g.sample_size(20);
    g.bench_function("build_10k", |bch| {
        bch.iter(|| {
            let mut idx = Bm25Index::new();
            for (i, t) in docs.iter().enumerate() {
                idx.add_chunk(i as u64, t);
            }
            idx
        })
    });
    let mut idx = Bm25Index::new();
    for (i, t) in docs.iter().enumerate() {
        idx.add_chunk(i as u64, t);
    }
    g.bench_function("score_query_10k", |bch| {
        bch.iter(|| idx.score_query(black_box("R000123 turbine pressure")))
    });
    g.finish();
}

/// Branch creation shares the base's chunks (Arc) but copies its BM25 index,
/// so cost grows with the base. Each branch is removed (empty merge + delete)
/// outside the timed region. `branch_pipe` logs one line per call.
fn branch_pipe(c: &mut Criterion) {
    let mut g = c.benchmark_group("branch_pipe");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(3));
    let counter = AtomicU64::new(0);
    for n in [1_000, 10_000] {
        let base = format!("bench_base_{n}");
        let bus = filled_bus(&base, n, 384, VectorMode::HotSimd, 0);
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bch, _| {
            bch.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let name = format!("bench_branch_{}", counter.fetch_add(1, Ordering::Relaxed));
                    let start = Instant::now();
                    bus.branch_pipe(&base, &name).expect("branch");
                    total += start.elapsed();
                    bus.merge_pipe(&base, &name, true).expect("remove branch");
                }
                total
            })
        });
    }
    g.finish();
}

fn mode_c(c: &mut Criterion) {
    let mut g = c.benchmark_group("mode_c_search");
    g.sample_size(30);
    let (n, dim) = (5_000, 384);
    let query = unit_vector(999_999, dim);
    let mode_a = filled_bus("bench_mode_a", n, dim, VectorMode::HotSimd, 0);
    let mode_c = filled_bus("bench_mode_c", n, dim, VectorMode::Sq8Nvme, 0);
    g.throughput(Throughput::Elements(n as u64));
    for (label, bus, pipe) in [
        ("mode_a_f32", &mode_a, "bench_mode_a"),
        ("mode_c_sq8_mmap", &mode_c, "bench_mode_c"),
    ] {
        g.bench_function(label, |bch| {
            bch.iter(|| {
                bus.search_pipe(pipe, &query, "turbine pressure", 5, None, None, false, true)
            })
        });
    }
    g.finish();
}

fn chunking(c: &mut Criterion) {
    let mut g = c.benchmark_group("chunk_text");
    let mut doc = String::new();
    let mut i = 0;
    while doc.len() < 10 * 1024 {
        doc.push_str(&record_text(i));
        doc.push_str(if i % 5 == 4 { ".\n\n" } else { ". " });
        i += 1;
    }
    g.throughput(Throughput::Bytes(doc.len() as u64));
    for strategy in [
        "sliding_window",
        "sentence_aware",
        "sentence_window",
        "paragraph",
    ] {
        g.bench_function(strategy, |bch| {
            bch.iter(|| chunk_text(black_box(&doc), strategy, 100, 20))
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    dot_product,
    search_pipe,
    search_filtered,
    bm25,
    branch_pipe,
    mode_c,
    chunking
);

fn main() {
    let dir = std::env::temp_dir().join(format!("ore-bench-{}", std::process::id()));
    // SAFETY: runs before any benchmark, while this is the only thread.
    unsafe { std::env::set_var("ORE_DIR", &dir) };
    benches();
    Criterion::default().configure_from_args().final_summary();
    let _ = std::fs::remove_dir_all(&dir);
}
