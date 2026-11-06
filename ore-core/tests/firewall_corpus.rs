//! Runs the labelled adversarial corpus (`examples/ipc_mmu/corpus/adversarial.json`)
//! through `ContextFirewall::secure_retrieved_context`, the screen Auto-RAG applies to
//! every retrieved chunk.
//!
//! `drop`, `redact`, and `pass` cases are asserted. `log_only`, `known_evasion`, and
//! `known_false_positive` cases document current limits: they are reported, not
//! asserted, and the report says when one of them starts behaving differently.
//!
//! `cargo test -p ore-core --test firewall_corpus -- --nocapture` prints the report.

use ore_common::manifest::AppManifest;
use ore_core::firewall::ContextFirewall;
use serde_json::Value;

const CORPUS: &str = include_str!("../../examples/ipc_mmu/corpus/adversarial.json");

/// The reader in the corpus: PII redaction on, no shell, not a coding agent, no network.
fn reader() -> AppManifest {
    let mut m = AppManifest {
        app_id: "firewall_corpus_reader".to_string(),
        ..Default::default()
    };
    m.privacy.enforce_pii_redaction = true;
    m
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Dropped,
    Redacted,
    Unchanged,
}

fn outcome(input: &str, screened: &Option<String>) -> Outcome {
    match screened {
        None => Outcome::Dropped,
        Some(text) if text == input => Outcome::Unchanged,
        Some(_) => Outcome::Redacted,
    }
}

#[test]
fn adversarial_corpus() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("adversarial.json is valid JSON");
    let cases = corpus["cases"].as_array().expect("cases array");
    let manifest = reader();

    let mut failures = Vec::new();
    let mut report = Vec::new();
    let mut counts = std::collections::BTreeMap::<String, (usize, usize)>::new();

    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expect = case["expect"].as_str().unwrap();
        let text = case["text"].as_str().unwrap();
        let screened = ContextFirewall::secure_retrieved_context(&manifest, text);
        let got = outcome(text, &screened);

        let ok = match expect {
            "drop" => got == Outcome::Dropped,
            "redact" => {
                let leaked: Vec<&str> = case["secrets"]
                    .as_array()
                    .map(|s| s.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let leaked: Vec<&str> = leaked
                    .into_iter()
                    .filter(|s| screened.as_deref().is_some_and(|t| t.contains(s)))
                    .collect();
                got == Outcome::Redacted && leaked.is_empty()
            }
            "pass" => got == Outcome::Unchanged,
            // Documented behaviour: a Medium-severity rule logs and lets the chunk through.
            "log_only" => got == Outcome::Unchanged,
            // A known gap "holds" while the firewall still misses it.
            "known_evasion" => got == Outcome::Unchanged,
            "known_false_positive" => got != Outcome::Unchanged,
            other => panic!("{id}: unknown expectation '{other}'"),
        };

        let entry = counts.entry(expect.to_string()).or_default();
        entry.0 += 1;
        if ok {
            entry.1 += 1;
        }

        let asserted = matches!(expect, "drop" | "redact" | "pass");
        let status = match (asserted, ok) {
            (true, true) => "ok",
            (true, false) => "FAIL",
            (false, true) => "gap holds",
            (false, false) => "CHANGED (update the label)",
        };
        report.push(format!("{id:<5} {expect:<21} {got:<10?} {status}"));
        if asserted && !ok {
            failures.push(format!("{id} ({expect}): got {got:?} -> {screened:?}"));
        }
    }

    println!(
        "\nRetrieved-context firewall, {} labelled cases:",
        cases.len()
    );
    for line in &report {
        println!("  {line}");
    }
    println!("\nBy expectation (matching / total):");
    for (expect, (total, matching)) in &counts {
        println!("  {expect:<21} {matching}/{total}");
    }

    assert!(
        failures.is_empty(),
        "firewall regressions:\n{}",
        failures.join("\n")
    );
}
