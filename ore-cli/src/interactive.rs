use colored::*;
use inquire::{Confirm, CustomType, MultiSelect, Select, Text};
use reqwest::Client;
use std::fs;
use std::process::exit;

use crate::utils::{get_ore_dir, get_ore_theme, print_panel, print_section_divider};

pub fn run_init_wizard() {
    // 1. ASCII Art Branding
    let logo = r#"
      ██████╗ ██████╗ ███████╗
     ██╔═══██╗██╔══██╗██╔════╝
     ██║   ██║██████╔╝█████╗  
     ██║   ██║██╔══██╗██╔══╝  
     ╚██████╔╝██║  ██║███████╗
      ╚═════╝ ╚═╝  ╚═╝╚══════╝
    ██╗  ██╗███████╗██████╗ ███╗   ██╗███████╗██╗     
    ██║ ██╔╝██╔════╝██╔══██╗████╗  ██║██╔════╝██║     
    █████╔╝ █████╗  ██████╔╝██╔██╗ ██║█████╗  ██║     
    ██╔═██╗ ██╔══╝  ██╔══██╗██║╚██╗██║██╔══╝  ██║     
    ██║  ██╗███████╗██║  ██║██║ ╚████║███████╗███████╗
    ╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝╚══════╝╚══════╝"#;

    println!("{}", logo.bright_cyan().bold());
    println!(
        " Open Runtime Environment • ORE Kernel v0.1.0-alpha • The Operating System for Local AI"
    );

    print_panel(
        "System Initialization",
        "Configure global system parameters for agents",
    );

    let theme = get_ore_theme();

    let engines = vec![
        "Native (Bare-metal Rust execution, maximum control)",
        "Ollama (Background daemon, easiest setup)",
    ];

    let engine_selection = Select::new("Select your primary AI Execution Engine:", engines)
        .with_render_config(theme)
        .prompt()
        .unwrap_or_else(|_| exit(0));

    let embedders = vec![
        "all-minilm      (Fast & Lightweight, 90MB - Best for laptops)",
        "system-embedder (Nomic v1.5, High Accuracy, 500MB - Best for desktops)",
    ];

    let embedder_selection = Select::new("Select your Semantic Bus Embedder:", embedders)
        .with_render_config(theme)
        .prompt()
        .unwrap_or_else(|_| exit(0));

    let selected_embedder = if embedder_selection.starts_with("all-minilm") {
        "all-minilm"
    } else {
        "system-embedder"
    };

    let mut toml_output = String::new();
    toml_output.push_str("[system]\n");

    if engine_selection.starts_with("Ollama") {
        // OLLAMA SETUP
        toml_output.push_str("engine = \"ollama\"\n");
        toml_output.push_str(&format!("embedder = \"{}\"\n\n", selected_embedder));
        toml_output.push_str("[ollama]\n");

        let url = Text::new("Enter Ollama API URL:")
            .with_default("http://127.0.0.1:11434")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_else(|_| exit(0));

        toml_output.push_str(&format!("url = \"{}\"\n", url));
    } else {
        // NATIVE SETUP
        toml_output.push_str("engine = \"native\"\n");
        toml_output.push_str(&format!("embedder = \"{}\"\n\n", selected_embedder));
        toml_output.push_str("[native]\n");

        let model = Text::new("Default Model Alias (e.g., qwen2.5:0.5b):")
            .with_default("qwen2.5:0.5b")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_else(|_| exit(0));

        toml_output.push_str(&format!("default_model = \"{}\"\n\n", model));
    }

    print_panel(
        "Garbage Collection (GC)",
        "How long should the OS keep idle Agent data in RAM?",
    );

    let cache_ttl = CustomType::<u64>::new("Mathematical Cache TTL (hours) [0 = Infinite]:")
        .with_default(24)
        .with_render_config(theme)
        .prompt()
        .unwrap_or_else(|_| exit(0));

    let pipe_ttl = CustomType::<u64>::new("Semantic Pipe TTL (hours) [0 = Infinite]:")
        .with_default(32)
        .with_render_config(theme)
        .prompt()
        .unwrap_or_else(|_| exit(0));

    toml_output.push_str("[memory]\n");
    toml_output.push_str(&format!("cache_ttl_hours = {}\n", cache_ttl));
    toml_output.push_str(&format!("pipe_ttl_hours = {}\n", pipe_ttl));

    // Save to the root directory
    let base_dir = get_ore_dir();
    let config_path = base_dir.join("ore.toml");
    fs::write(&config_path, toml_output).expect("Failed to write config file");

    println!("\n{} ORE System configured successfully!", "[OK]".green());
    println!("Configuration saved to: {}", "ore.toml".blue());
    println!("Please restart the 'ore-server' to apply changes.\n");
}

pub async fn run_manifest_wizard(app_id: &String, client: &Client) {
    let theme = get_ore_theme();

    print_panel(
        "Secure Manifest Forage",
        &format!("Target Agent: {}", app_id.cyan()),
    );

    let modules = vec![
        "Privacy      (PII Redaction)",
        "Resources    (GPU Quotas, Models, Paging)",
        "File System  (File System Boundaries)",
        "Network      (Egress Control)",
        "Execution    (WASM/Shell Sandbox)",
        "IPC          (Agent-to-Agent Swarm)",
    ];

    let selections = MultiSelect::new(
        "Select required sub-systems for this agent:",
        modules.clone(),
    )
    .with_help_message("Space to toggle, Enter to confirm")
    .with_render_config(theme)
    .with_formatter(&|answers| {
        answers
            .iter()
            .map(|ans| {
                // Split at the double spaces to isolate the short name
                ans.value.split("  ").next().unwrap_or(ans.value).trim()
            })
            .collect::<Vec<_>>()
            .join(", ")
    })
    .prompt()
    .unwrap_or_else(|_| exit(0));

    // println!("\nSelected modules:");
    // for i in &selections {
    //     println!("{}", i);
    // }

    if selections.is_empty() {
        println!(
            "\n{}  No sub-systems selected. Agent will be strictly air-gapped.",
            "⚠".yellow()
        );
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
    if selections.contains(&modules[0]) {
        print_section_divider("1", "Privacy");
        let pii = Confirm::new("Enforce PII Redaction (strip passwords/emails)?")
            .with_default(true)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(true);
        toml_output.push_str("[privacy]\n");
        toml_output.push_str(&format!("enforce_pii_redaction = {}\n\n", pii));
    }

    // --- 2. RESOURCES ---
    if selections.contains(&modules[1]) {
        print_section_divider("2", "Resources");

        let mut available_models = Vec::new();
        if let Ok(res) = client.get("http://127.0.0.1:6767/ls").send().await
            && let Ok(text) = res.text().await
        {
            for line in text.lines().skip(2) {
                if line.starts_with("No models") || line.is_empty() {
                    continue;
                }
                if let Some(model_name) = line.split('|').next() {
                    available_models.push(model_name.trim().to_string());
                }
            }
        }

        let selected_models_formatted = if available_models.is_empty() {
            println!(
                "{} No installed models detected. Type them manually.",
                "ℹ".blue()
            );
            println!(
                "       You can type them manually now, and install them later using 'ore pull <model>'."
            );

            let manual = Text::new("Allowed models (comma-separated):")
                .with_render_config(theme)
                .prompt()
                .unwrap_or_default();

            format_list(manual)
        } else {
            let selected = MultiSelect::new(
                "Select allowed models for this agent:",
                available_models.clone(),
            )
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();

            let quoted: Vec<String> = selected.into_iter().map(|s| format!("\"{}\"", s)).collect();
            format!("[{}]", quoted.join(", "))
        };

        let tokens = CustomType::<u32>::new("Max tokens per minute (Rate Limit):")
            .with_default(10000)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(10000);

        let priorities = vec!["low", "normal", "high"];
        let p_idx = Select::new("GPU Priority level:", priorities.clone())
            .with_render_config(theme)
            .prompt()
            .unwrap_or("normal");

        let mut json_history =
            Confirm::new("Enable JSON Chat History (Required for Memory Compaction)?")
                .with_default(true)
                .with_render_config(theme)
                .prompt()
                .unwrap_or(true);

        let paging = Confirm::new("Enable Stateful Paging (KV-Cache SSD Swap for long tasks)?")
            .with_default(false)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(false);

        if !json_history && paging {
            println!(
                "{} Stateful Paging requires JSON History. Enabling it automatically.",
                "⚠".yellow()
            );
            json_history = true;
        }

        toml_output.push_str("[resources]\n");
        toml_output.push_str(&format!("allowed_models = {}\n", selected_models_formatted));
        toml_output.push_str(&format!("max_tokens_per_minute = {}\n", tokens));
        toml_output.push_str(&format!("gpu_priority = \"{}\"\n", p_idx));
        toml_output.push_str(&format!("json_history = {}\n", json_history));
        toml_output.push_str(&format!("stateful_paging = {}\n\n", paging));

        if json_history {
            println!(
                "\n  {} {}",
                "ℹ".blue(),
                "Configuring Memory Limits (Compaction)".bright_black()
            );
            let max_tokens = CustomType::<u32>::new("Max Conversation Context (Tokens):")
                .with_default(8192)
                .with_render_config(theme)
                .prompt()
                .unwrap_or(8192);

            let max_kv_mb = if paging {
                CustomType::<u32>::new("Max Physical KV-Cache Size (MB):")
                    .with_default(1024)
                    .with_render_config(theme)
                    .prompt()
                    .unwrap_or(1024)
            } else {
                0
            };

            let auto_sum = Confirm::new("Auto-summarize when limits are reached?")
                .with_default(true)
                .with_render_config(theme)
                .prompt()
                .unwrap_or(true);

            toml_output.push_str("[memory_limits]\n");
            toml_output.push_str(&format!("max_json_tokens = {}\n", max_tokens));
            toml_output.push_str(&format!("max_kv_cache_mb = {}\n", max_kv_mb));
            toml_output.push_str(&format!("auto_summarize_on_cap = {}\n\n", auto_sum));
        }
    }

    // --- 3. FILE SYSTEM ---
    if selections.contains(&modules[2]) {
        print_section_divider("3", "File System");
        let read_paths = Text::new("Allowed READ paths (comma-separated):")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();
        let write_paths = Text::new("Allowed WRITE paths (comma-separated):")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();
        let max_mb = CustomType::<u32>::new("Max file size allowed (MB):")
            .with_default(5)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(5);

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
    if selections.contains(&modules[3]) {
        print_section_divider("4", "Network");
        let domains = Text::new("Allowed external domains (comma-separated):")
            .with_default("github.com, wikipedia.org")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();
        let localhost = Confirm::new("Allow LOCALHOST access? (High Risk for SSRF)")
            .with_default(false)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(false);

        toml_output.push_str("[network]\n");
        toml_output.push_str("network_enabled = true\n");
        toml_output.push_str(&format!("allowed_domains = {}\n", format_list(domains)));
        toml_output.push_str(&format!("allow_localhost_access = {}\n\n", localhost));
    }

    // --- 5. EXECUTION ---
    if selections.contains(&modules[4]) {
        print_section_divider("5", "Execution");
        println!("{} {}", "⚠".red(), "SECURITY WARNING".red().bold());
        let shell = Confirm::new("Allow raw SHELL execution? (Extreme Risk)")
            .with_default(false)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(false);
        let wasm = Confirm::new("Allow WebAssembly (WASM) Sandbox execution?")
            .with_default(true)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(true);
        let tools = Text::new("Allowed Agent Tools (comma-separated):")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();

        toml_output.push_str("[execution]\n");
        toml_output.push_str(&format!("can_execute_shell = {}\n", shell));
        toml_output.push_str(&format!("can_execute_wasm = {}\n", wasm));
        toml_output.push_str(&format!("allowed_tools = {}\n\n", format_list(tools)));
    }

    // --- 6. IPC ---
    if selections.contains(&modules[5]) {
        print_section_divider("6", "IPC (Inter-Process Communication)");
        let agents = Text::new("Tier 1: Allowed Agent targets (comma-separated):")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();
        let pipes = Text::new("Tier 2: Allowed Semantic Memory pipes (comma-separated):")
            .with_render_config(theme)
            .prompt()
            .unwrap_or_default();
        let persistence = Confirm::new("Enable Semantic Persistence (Flush Vectors to SSD)?")
            .with_default(true)
            .with_render_config(theme)
            .prompt()
            .unwrap_or(true);

        toml_output.push_str("[ipc]\n");
        toml_output.push_str(&format!(
            "allowed_agent_targets = {}\n",
            format_list(agents)
        ));
        toml_output.push_str(&format!(
            "allowed_semantic_pipes = {}\n",
            format_list(pipes)
        ));
        toml_output.push_str(&format!("semantic_persistence = {}\n\n", persistence));
    }

    // Write to disk
    let base_dir = get_ore_dir();
    let manifest_dir = base_dir.join("manifests");
    if !manifest_dir.exists() {
        fs::create_dir_all(&manifest_dir).unwrap();
    }
    let file_path = manifest_dir.join(format!("{}.toml", app_id));
    fs::write(&file_path, &toml_output).expect("Failed to write manifest");

    print_panel("Manifest Preview", "");
    let width: usize = 65;
    for line in toml_output.lines() {
        let plain_len = line.chars().count();
        // Calculate spaces needed to push the right border to exactly 75 chars
        let padding = " ".repeat(width.saturating_sub(plain_len + 4));

        // Micro Syntax-Highlighter for TOML
        let styled_line = if line.starts_with('[') && line.ends_with(']') {
            // Highlight [sections] in bold magenta
            line.cyan().bold().to_string()
        } else if let Some(idx) = line.find('=') {
            // Highlight key = value pairs
            let key = &line[..idx].trim_end();
            let val = &line[idx + 1..].trim_start();
            format!(
                "{} {} {}",
                key.bright_black(),
                "=".bright_black(),
                val.cyan()
            )
        } else {
            // Blank lines or plain text
            line.bright_black().to_string()
        };

        // Print left border, colored text, padding, right border
        println!(
            "{} {}{} {}",
            "│".bright_black(),
            styled_line,
            padding,
            "│".bright_black()
        );
    }

    // Perfectly sealed bottom border
    let bottom_filler = "─".repeat(width - 2);
    println!(
        "{}{}{}",
        "╰".bright_black(),
        bottom_filler.bright_black(),
        "╯".bright_black()
    );

    println!("\n{} Manifest forged successfully.", "✔".green());
    println!(
        "{} Path   :: {}",
        "ℹ".blue(),
        file_path.display().to_string().bright_black()
    );
    println!(
        "{} Status :: Awaiting Kernel Reboot for Enforcement.\n",
        "ℹ".blue()
    );
}
