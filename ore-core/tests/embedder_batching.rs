//! Batched embedding must give the same vectors as embedding each text alone.
//!
//! Runs against the embedders installed under `<ore_dir>/models`, so it is ignored
//! by default:
//!
//!     cargo test --release -p ore-core --test embedder_batching -- --ignored --nocapture
//!
//! For each installed embedder it embeds a varied set of texts one at a time and in
//! batches, checks every vector matches (max abs diff and cosine), checks order,
//! and prints the speedup.

use std::time::Instant;

use candle_core::Device;
use ore_core::driver::EmbedTask;
use ore_core::inference::native::models::{bert, nomic};

const MAX_ABS_DIFF: f32 = 1e-4;
const MIN_COSINE: f32 = 0.99999;

fn texts() -> Vec<String> {
    let mut texts = vec![
        String::new(),
        "ok".to_string(),
        "Incident INC-4471: the payments gateway returned ERR_7731 after a TLS certificate expired.".to_string(),
        "数据库分片 pg-users-03 的磁盘使用率达到百分之九十五。".to_string(),
        "Le service de facturation à Montréal a échoué après la mise à jour 2.3.1.".to_string(),
        "Rust panic in ingest-svc at src/parser/frame.rs:88:31 on a zero-length frame.".to_string(),
    ];
    // Many lengths, so batches mix and pad.
    for i in 0..60 {
        let words = 3 + (i * 7) % 90;
        texts.push(
            (0..words)
                .map(|w| {
                    [
                        "memory", "agent", "kernel", "shard", "replica", "token", "vector",
                    ][(i + w) % 7]
                })
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    // Longer than BERT's 512 positions: must be truncated, not crash.
    texts.push("the replica lag stayed below one second during the drill ".repeat(80));
    // Duplicates must embed identically.
    texts.push(texts[2].clone());
    texts
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

fn check(name: &str, embed: impl Fn(Vec<String>) -> anyhow::Result<Vec<Vec<f32>>>) {
    let texts = texts();

    let start = Instant::now();
    let single: Vec<Vec<f32>> = texts
        .iter()
        .map(|t| embed(vec![t.clone()]).expect("single embed").remove(0))
        .collect();
    let single_s = start.elapsed().as_secs_f64();

    let start = Instant::now();
    let batched = embed(texts.clone()).expect("batched embed");
    let batched_s = start.elapsed().as_secs_f64();

    assert_eq!(batched.len(), texts.len(), "{name}: one vector per input");
    let mut worst_diff = 0f32;
    let mut worst_cos = 1f32;
    for (i, (a, b)) in single.iter().zip(&batched).enumerate() {
        assert_eq!(a.len(), b.len(), "{name}: dimension of text {i}");
        let diff = a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0f32, f32::max);
        let cos = cosine(a, b);
        worst_diff = worst_diff.max(diff);
        worst_cos = worst_cos.min(cos);
        assert!(
            diff <= MAX_ABS_DIFF && cos >= MIN_COSINE,
            "{name}: text {i} ({:?}) differs when batched: max abs diff {diff}, cosine {cos}",
            &texts[i].chars().take(40).collect::<String>()
        );
        let norm: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-3,
            "{name}: text {i} is not unit length ({norm})"
        );
    }
    let dup = texts.len() - 1;
    assert_eq!(
        batched[dup], batched[2],
        "{name}: identical texts must embed identically"
    );

    println!(
        "{name}: {} texts, dim {} | one at a time {:.2}s, batched {:.2}s ({:.1}x) | worst abs diff {:.2e}, worst cosine {:.7}",
        texts.len(),
        batched[0].len(),
        single_s,
        batched_s,
        single_s / batched_s,
        worst_diff,
        worst_cos
    );
}

type Embed<'a> = Box<dyn Fn(Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> + 'a>;

/// Calls `f` with every BERT or NomicBert embedder installed under <ore_dir>/models.
fn for_each_embedder(mut f: impl FnMut(&str, Embed)) {
    let models = ore_common::paths::get_models_dir();
    let device = Device::Cpu;
    let mut ran = 0;

    for dir in std::fs::read_dir(&models).expect("models dir").flatten() {
        let path = dir.path();
        let Ok(config) = std::fs::read_to_string(path.join("config.json")) else {
            continue;
        };
        if !path.join("model.safetensors").exists() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let path_str = path.to_string_lossy().to_string();

        if config.contains("NomicBertModel") {
            let model = nomic::SystemEmbedder::load(&path_str, &device).expect("load nomic");
            f(
                &name,
                Box::new(move |t| model.embed_batch(t, EmbedTask::Document)),
            );
            ran += 1;
        } else if config.contains("\"BertModel\"") {
            let model = bert::SystemEmbedder::load(&path_str, &device).expect("load bert");
            f(&name, Box::new(move |t| model.embed_batch(t)));
            ran += 1;
        }
    }
    assert!(
        ran > 0,
        "no BERT or NomicBert embedder found in {}",
        models.display()
    );
}

#[test]
#[ignore = "needs embedders under <ore_dir>/models; run with --ignored"]
fn batched_embeddings_match_single_embeddings() {
    for_each_embedder(|name, embed| check(name, embed));
}

/// The workload that matters for /ipc/share: hundreds of short records.
#[test]
#[ignore = "needs embedders under <ore_dir>/models; run with --ignored"]
fn throughput_on_short_records() {
    let records: Vec<String> = (0..256)
        .map(|i| {
            format!(
                "Record R{i:06}: the {} sensor at station {} reported {} units during the night shift, filed under batch B{} for later review.",
                ["amber", "cobalt", "silver", "tidal"][i % 4],
                ["Arden", "Elgin", "Jarrow"][i % 3],
                (i * 7919) % 10_000,
                100 + i % 900
            )
        })
        .collect();

    for_each_embedder(|name, embed| {
        let _ = embed(records[..4].to_vec()); // warm up
        let start = Instant::now();
        for r in &records {
            embed(vec![r.clone()]).expect("single");
        }
        let single = start.elapsed().as_secs_f64();
        let start = Instant::now();
        embed(records.clone()).expect("batched");
        let batched = start.elapsed().as_secs_f64();
        println!(
            "{name}: {} short records | one at a time {:.1} texts/s, batched {:.1} texts/s ({:.1}x)",
            records.len(),
            records.len() as f64 / single,
            records.len() as f64 / batched,
            single / batched
        );
    });
}

/// Every NomicBert embedder installed under <ore_dir>/models.
fn nomic_embedders() -> Vec<(String, nomic::SystemEmbedder)> {
    let models = ore_common::paths::get_models_dir();
    let found: Vec<_> = std::fs::read_dir(&models)
        .expect("models dir")
        .flatten()
        .map(|d| d.path())
        .filter(|p| {
            p.join("model.safetensors").exists()
                && std::fs::read_to_string(p.join("config.json"))
                    .is_ok_and(|c| c.contains("NomicBertModel"))
        })
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let model = nomic::SystemEmbedder::load(&p.to_string_lossy(), &Device::Cpu)
                .expect("load nomic");
            (name, model)
        })
        .collect();
    assert!(
        !found.is_empty(),
        "no NomicBert embedder in {}",
        models.display()
    );
    found
}

/// Queries and documents get their own task prefix, and with them each query finds
/// its own document.
#[test]
#[ignore = "needs a NomicBert embedder under <ore_dir>/models; run with --ignored"]
fn nomic_task_prefixes() {
    let docs = [
        "To rotate the TLS certificate, generate a new key pair, upload it to the load balancer, and restart the gateway.",
        "The billing service retries failed card payments three times over 48 hours before cancelling the subscription.",
        "Replica lag on pg-users-03 is caused by long-running analytics queries holding back vacuum.",
        "Kubernetes evicts pods when node memory pressure crosses the eviction threshold.",
    ];
    let queries = [
        "how do I replace an expiring certificate?",
        "what happens when a customer's card payment fails?",
        "why is the database replica falling behind?",
        "why were my pods killed when the node ran low on RAM?",
    ];

    for (name, model) in nomic_embedders() {
        let text = vec!["database replication".to_string()];
        let as_query = model.embed_batch(text.clone(), EmbedTask::Query).unwrap();
        let as_doc = model.embed_batch(text, EmbedTask::Document).unwrap();
        let same = cosine(&as_query[0], &as_doc[0]);
        assert!(
            same < 0.999,
            "{name}: query and document prefixes gave the same vector"
        );

        let doc_vecs = model
            .embed_batch(
                docs.iter().map(|s| s.to_string()).collect(),
                EmbedTask::Document,
            )
            .unwrap();
        let query_vecs = model
            .embed_batch(
                queries.iter().map(|s| s.to_string()).collect(),
                EmbedTask::Query,
            )
            .unwrap();
        for (qi, q) in query_vecs.iter().enumerate() {
            let best = doc_vecs
                .iter()
                .enumerate()
                .max_by(|a, b| cosine(q, a.1).total_cmp(&cosine(q, b.1)))
                .unwrap()
                .0;
            assert_eq!(
                best, qi,
                "{name}: query {:?} retrieved doc {best}",
                queries[qi]
            );
        }
        println!(
            "{name}: prefixes ok (query vs document cosine {same:.4}), 4/4 queries retrieve their doc"
        );
    }
}

/// Input is cut at nomic::MAX_SEQ_LEN tokens: anything after that point cannot change
/// the vector, and a very long text embeds without an 8192-token attention spike.
#[test]
#[ignore = "needs a NomicBert embedder under <ore_dir>/models; run with --ignored"]
fn nomic_truncates_at_max_seq_len() {
    let words = [
        "replica", "lag", "vacuum", "shard", "kernel", "token", "agent", "memory",
    ];
    // ~3000 tokens: past 2048, well inside the model's 8192 positions.
    let long: String = (0..3000)
        .map(|i| words[i % words.len()])
        .collect::<Vec<_>>()
        .join(" ");
    let longer = format!(
        "{long} {}",
        "completely different trailing content about invoices ".repeat(400)
    );

    for (name, model) in nomic_embedders() {
        let start = Instant::now();
        let v = model
            .embed_batch(vec![long.clone(), longer.clone()], EmbedTask::Document)
            .unwrap();
        let diff = v[0]
            .iter()
            .zip(&v[1])
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            diff <= MAX_ABS_DIFF,
            "{name}: text past {} tokens changed the vector (max abs diff {diff})",
            nomic::MAX_SEQ_LEN
        );
        println!(
            "{name}: truncated at {} tokens, 2 long texts in {:.2}s",
            nomic::MAX_SEQ_LEN,
            start.elapsed().as_secs_f64()
        );
    }
}
