use colored::*;
use dialoguer::{Confirm, Input, MultiSelect, Select, theme::SimpleTheme};
use reqwest::Client;
use std::fs;
use std::path::Path;

#[derive(serde::Deserialize)]
struct DriverTagsResponse {
    models: Vec<DriverModel>,
}

#[derive(serde::Deserialize)]
struct DriverModel {
    name: String,
}

pub fn run_init_wizard() {
    println!("\n\n ORE KERNEL :: SYSTEM INITIALIZATION\n\n");

    let engines = &[
        "Ollama (Background daemon, easiest setup)",
        "Native (Bare-metal Rust execution, maximum control)",
    ];

    let engine_idx = Select::with_theme(&SimpleTheme)
        .with_prompt("Select your primary AI Inference Engine")
        .default(0)
        .items(engines)
        .interact()
        .unwrap();

    let embedders = &[
        "all-minilm (Fast & Lightweight, 90MB - Best for laptops)",
        "system-embedder (Nomic v1.5, High Accuracy, 500MB - Best for desktops)",
    ];

    let embedder_idx = Select::with_theme(&SimpleTheme)
        .with_prompt("Select your System Embedder (Semantic Bus Engine)")
        .default(0)
        .items(embedders)
        .interact()
        .unwrap();

    let selected_embedder = if embedder_idx == 0 {
        "all-minilm"
    } else {
        "system-embedder"
    };

    let mut toml_output = String::new();
    toml_output.push_str("[system]\n");

    if engine_idx == 0 {
        // OLLAMA SETUP
        toml_output.push_str("engine = \"ollama\"\n");
        toml_output.push_str(&format!("embedder = \"{}\"\n\n", selected_embedder));
        toml_output.push_str("[ollama]\n");

        let url: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Enter Ollama API URL")
            .default("http://127.0.0.1:11434".into())
            .interact_text()
            .unwrap();

        toml_output.push_str(&format!("url = \"{}\"\n", url));
    } else {
        // NATIVE SETUP
        toml_output.push_str("engine = \"native\"\n");
        toml_output.push_str(&format!("embedder = \"{}\"\n\n", selected_embedder));
        toml_output.push_str("[native]\n");

        let model: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Enter default model alias (e.g., qwen2.5:0.5b)")
            .default("qwen2.5:0.5b".into())
            .interact_text()
            .unwrap();

        toml_output.push_str(&format!("default_model = \"{}\"\n\n", model));
    }

    println!("\n>>> CONFIGURING: RAM GARBAGE COLLECTION (GC)");
    println!("    (How long should the OS keep idle Agent data in RAM?)");

    let cache_ttl: u64 = Input::with_theme(&SimpleTheme)
        .with_prompt("Mathematical Cache TTL in hours [0 = Infinite]:")
        .default(24)
        .interact_text()
        .unwrap();

    let pipe_ttl: u64 = Input::with_theme(&SimpleTheme)
        .with_prompt("Semantic Pipe TTL in hours [0 = Infinite]:")
        .default(32)
        .interact_text()
        .unwrap();

    toml_output.push_str("[memory]\n");
    toml_output.push_str(&format!("cache_ttl_hours = {}\n", cache_ttl));
    toml_output.push_str(&format!("pipe_ttl_hours = {}\n", pipe_ttl));

    // Save to the root directory
    fs::write("../ore.toml", toml_output).expect("Failed to write config file");

    println!("\n{} ORE System configured successfully!", "[OK]".green());
    println!("Configuration saved to: {}", "ore.toml".blue());
    println!("Please restart the 'ore-server' to apply changes.\n");
}

pub async fn run_manifest_wizard(app_id: &String, client: &Client) {
    println!("\n ORE KERNEL :: SECURE MANIFEST FORGE");
    println!(" Target agent :: {}", app_id);
    println!(" Use [SPACE] to toggle modules, [ENTER] to confirm.\n");

    struct Module {
        name: &'static str,
        label: &'static str,
    }

    let modules = [
        Module {
            name: "Privacy",
            label: "Privacy      [ PII Redaction ]",
        },
        Module {
            name: "Resources",
            label: "Resources    [ GPU Quotas & Models ]",
        },
        Module {
            name: "File System",
            label: "File System  [ File System Boundaries ]",
        },
        Module {
            name: "Network",
            label: "Network      [ Network Egress Control ]",
        },
        Module {
            name: "Execution",
            label: "Execution    [ WASM/Shell Sandbox ]",
        },
        Module {
            name: "IPC",
            label: "IPC          [ Agent-to-Agent Swarm ]",
        },
    ];

    let labels: Vec<&str> = modules.iter().map(|m| m.label).collect();

    let selections = MultiSelect::with_theme(&SimpleTheme)
        .with_prompt("Select all the required sub-systems")
        .items(&labels)
        .interact()
        .unwrap();

    println!("\nSelected modules:");
    for i in &selections {
        println!("{}", modules[*i].name);
    }

    if selections.is_empty() {
        println!("\n[WARN] NO SUB-SYSTEMS SELECTED. AGENT WILL BE STRICTLY AIR-GAPPED.");
    }

    let format_list = |input: String| -> String {
        if input.trim().is_empty() {
            return "[]".to_string();
        }
        let items: Vec<String> = input
            .split(',')
            .map(|s| format!("\"{}\"", s.trim()))
            .collect();
        format!("[{}]", items.join(", "))
    };

    // Build TOML string dynamically
    let mut toml_output = format!("app_id = \"{}\"\n", app_id);
    toml_output.push_str("description = \"Generated by ORE CLI\"\n");
    toml_output.push_str("version = \"1.0.0\"\n\n");

    // --- 1. PRIVACY ---
    if selections.contains(&0) {
        println!("\n>>> CONFIGURING: Privacy");
        let pii = Confirm::with_theme(&SimpleTheme)
            .with_prompt("Enforce PII Redaction (strip passwords/emails)?")
            .default(true)
            .interact()
            .unwrap();
        toml_output.push_str("[privacy]\n");
        toml_output.push_str(&format!("enforce_pii_redaction = {}\n\n", pii));
    }

    // --- 2. RESOURCES ---
    if selections.contains(&1) {
        println!("\n>>> CONFIGURING: Resources");

        let mut available_models = Vec::new();
        if let Ok(res) = client.get("http://127.0.0.1:11434/api/tags").send().await
            && let Ok(tags) = res.json::<DriverTagsResponse>().await
        {
            available_models = tags.models.into_iter().map(|m| m.name).collect();
        }

        let selected_models_formatted;

        if available_models.is_empty() {
            println!(
                "{} No installed models detected, or Driver is offline.",
                "[WARN]".yellow()
            );
            println!(
                "       You can type them manually now, and install them later using 'ore pull <model>'."
            );

            let manual: String = Input::with_theme(&SimpleTheme)
                .with_prompt("Allowed models (comma-separated, e.g., qwen2.5:0.5b)")
                .default("".into())
                .interact_text()
                .unwrap();

            selected_models_formatted = format_list(manual);
        } else {
            let selection_indices = MultiSelect::with_theme(&SimpleTheme)
                .with_prompt("Select allowed models for this agent")
                .items(&available_models)
                .interact()
                .unwrap();

            if selection_indices.is_empty() {
                println!(
                    "{} No models selected. Agent will have no brain!",
                    "[WARN]".yellow()
                );
                selected_models_formatted = "[]".to_string();
            } else {
                let selected: Vec<String> = selection_indices
                    .into_iter()
                    .map(|i| format!("\"{}\"", available_models[i]))
                    .collect();
                selected_models_formatted = format!("[{}]", selected.join(", "));
            }
        }

        let tokens: u32 = Input::with_theme(&SimpleTheme)
            .with_prompt("Max tokens per minute (Rate Limit)")
            .default(10000)
            .interact_text()
            .unwrap();

        let priorities = &["low", "normal", "high"];
        let p_idx = Select::with_theme(&SimpleTheme)
            .with_prompt("GPU Priority level")
            .default(1)
            .items(priorities)
            .interact()
            .unwrap();
        let paging = Confirm::with_theme(&SimpleTheme)
            .with_prompt("Enable Stateful Paging (KV-Cache SSD Swap for long tasks)?")
            .default(false)
            .interact()
            .unwrap();

        toml_output.push_str("[resources]\n");
        toml_output.push_str(&format!("allowed_models = {}\n", selected_models_formatted));
        toml_output.push_str(&format!("max_tokens_per_minute = {}\n", tokens));
        toml_output.push_str(&format!("gpu_priority = \"{}\"\n\n", priorities[p_idx]));
        toml_output.push_str(&format!("stateful_paging = {}\n\n", paging));
    }

    // --- 3. FILE SYSTEM ---
    if selections.contains(&2) {
        println!("\n>>> CONFIGURING: File System");
        let read_paths: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Allowed READ paths (comma-separated, leave blank for none)")
            .default("".into())
            .interact_text()
            .unwrap();

        let write_paths: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Allowed WRITE paths (comma-separated, leave blank for none)")
            .default("".into())
            .interact_text()
            .unwrap();

        let max_mb: u32 = Input::with_theme(&SimpleTheme)
            .with_prompt("Max file size allowed to read (MB)")
            .default(5)
            .interact_text()
            .unwrap();

        toml_output.push_str("[file_system]\n");
        toml_output.push_str(&format!(
            "allowed_read_paths = {}\n",
            format_list(read_paths)
        ));
        toml_output.push_str(&format!(
            "allowed_write_paths = {}\n",
            format_list(write_paths)
        ));
        toml_output.push_str(&format!("max_file_size_mb = {}\n\n", max_mb));
    }

    // --- 4. NETWORK ---
    if selections.contains(&3) {
        println!("\n>>> CONFIGURING: Network");
        let domains: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Allowed external domains (comma-separated)")
            .default("github.com, wikipedia.org".into())
            .interact_text()
            .unwrap();

        let localhost = Confirm::with_theme(&SimpleTheme)
            .with_prompt("Allow LOCALHOST access? (WARNING: High Risk for SSRF Attacks)")
            .default(false)
            .interact()
            .unwrap();

        toml_output.push_str("[network]\n");
        toml_output.push_str("network_enabled = true\n");
        toml_output.push_str(&format!("allowed_domains = {}\n", format_list(domains)));
        toml_output.push_str(&format!("allow_localhost_access = {}\n\n", localhost));
    }

    // --- 5. EXECUTION ---
    if selections.contains(&4) {
        println!("\n>>> CONFIGURING: Execution");
        let shell = Confirm::with_theme(&SimpleTheme)
            .with_prompt("Allow raw SHELL execution? (WARNING: Extreme Risk)")
            .default(false)
            .interact()
            .unwrap();

        let wasm = Confirm::with_theme(&SimpleTheme)
            .with_prompt("Allow WebAssembly (WASM) Sandbox execution?")
            .default(true)
            .interact()
            .unwrap();

        let tools: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Allowed Agent Tools (comma-separated, e.g., git_commit, file_search)")
            .default("".into())
            .interact_text()
            .unwrap();

        toml_output.push_str("[execution]\n");
        toml_output.push_str(&format!("can_execute_shell = {}\n", shell));
        toml_output.push_str(&format!("can_execute_wasm = {}\n", wasm));
        toml_output.push_str(&format!("allowed_tools = {}\n\n", format_list(tools)));
    }

    // --- 6. IPC ---
    if selections.contains(&5) {
        println!("\n>>> CONFIGURING: IPC");

        let agents: String = Input::with_theme(&SimpleTheme)
            .with_prompt(
                "Tier 1: Allowed Agent-to-Agent text targets (comma-separated, e.g., writer_agent)",
            )
            .default("".into())
            .interact_text()
            .unwrap();

        let pipes: String = Input::with_theme(&SimpleTheme)
            .with_prompt("Tier 2: Allowed Semantic Memory pipes (comma-separated, e.g., rust_docs)")
            .default("".into())
            .interact_text()
            .unwrap();

        toml_output.push_str("[ipc]\n");
        toml_output.push_str(&format!(
            "allowed_agent_targets = {}\n",
            format_list(agents)
        ));
        toml_output.push_str(&format!(
            "allowed_semantic_pipes = {}\n\n",
            format_list(pipes)
        ));
    }

    // Write to disk
    let file_path = format!("../manifests/{}.toml", app_id);
    if !Path::new("../manifests").exists() {
        fs::create_dir_all("../manifests").unwrap();
    }

    fs::write(&file_path, &toml_output).expect("Failed to write manifest");

    println!("\n==================================================");
    println!("[OK] MANIFEST FORGED SUCCESSFULLY.");
    println!("PATH   :: {}", file_path);
    println!("STATUS :: AWAITING KERNEL REBOOT FOR ENFORCEMENT.");
    println!("==================================================\n");

    println!("Preview:\n{}", toml_output);
}
