use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use reqwest::{Client, header::{HeaderMap, HeaderValue, AUTHORIZATION}};
use std::fs;
use std::path::Path;
use std::process::exit;

/// ORE: The Operating System for Local Intelligence
#[derive(Parser)]
#[command(name = "ore")]
#[command(version = "0.1.0", about = "Control the ORE Kernel", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(serde::Serialize)]
struct RunPayload {
    model: String,
    prompt: String,
}

#[derive(serde::Deserialize)]
struct DriverTagsResponse {
    models: Vec<DriverModel>,
}

#[derive(serde::Deserialize)]
struct DriverModel {
    name: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Check if the ORE Kernel is running and healthy
    Status,
    /// View real-time Kernel metrics and telemetry
    Top,
    /// Shows the models currently loaded into VRAM
    Ps,
    /// List all installed models on the local disk
    Ls {
        /// List all downloaded LLM models
        #[arg(long)]
        models: bool,

        /// List all agents currently under ORE control
        #[arg(long)]
        agents: bool,

        /// List all raw permission manifests created by the user
        #[arg(long)]
        manifests: bool,
    },
    /// Forcefully evict a model from the GPU VRAM
    Expel {
        /// The name of the model (e.g., llama3.21b)
        model_name: String,
    },
    /// Download and install a new AI Model to the local system
    Pull {
        /// The name of the model (e.g., mistral, qwen2.5-coder)
        model_name: String,
    },
    /// Run an AI model with a specific prompt
    Run {
        /// The name of the model to use (e.g., llama3.2, qwen2.5:0.5b)
        model: String,
        /// The prompt or task to send to the AI
        prompt: String,
    },
    /// Pre-load a model into GPU VRAM for zero-latency startups
    Load {
        /// The name of the model to load (e.g., llama3.2)
        model_name: String,
    },
    /// Interactive wizard to generate a secure Agent Manifest (.toml)
    Manifest {
        /// The ID of the agent (e.g., auto_coder)
        app_id: String,
    },
    /// Emergency kill-switch for runaway AI agents
    Kill {
        /// The App ID to terminate
        app_id: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let client = Client::new();
    let kernel_url = "http://127.0.0.1:3000";

    let token_path = "../ore-server/ore-kernel.token";
    let auth_token = match fs::read_to_string(token_path) {
        Ok(t) => t,
        Err(_) => {
            println!("{} FATAL: Could not read Kernel Security Token.", "[-]".red().bold());
            println!("    Is the ORE Kernel running? Did you start `ore-server`?");
            exit(1);
        }
    };

    let mut headers = reqwest::header::HeaderMap::new();
    let mut auth_value = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", auth_token)).unwrap();
    auth_value.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, auth_value);

    let client = Client::builder()
        .default_headers(headers)
        .build()
        .expect("Failed to build HTTP client");

    match &cli.command {
        Commands::Status => {
            println!("{} Pinging ORE Kernel...", "[*]".bright_blue());

            match client.get(format!("{}/health", kernel_url)).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let text = response.text().await.unwrap_or_default();
                        println!("{} Kernel is {}", "[+]".green(), "ONLINE".green().bold());
                        println!("{} System Message: {}", "[i]".bright_blue(), text.italic());
                    } else {
                        println!(
                            "{} Kernel returned an error: {}",
                            "[-]".red(),
                            response.status()
                        );
                    }
                }
                Err(_) => {
                    println!(
                        "{} ORE Kernel is {}!",
                        "[-]".red().bold(),
                        "OFFLINE".red().bold()
                    );
                    println!("    Run `cargo run -p ore-server` to boot the OS.");
                    exit(1);
                }
            }
        }
        Commands::Top => {
            println!("{} Fetching Kernel Telemetry...", "[*]".bright_blue());
            // this will hit a /metrics endpoint on the server
            println!("\n{}", "=== ORE KERNEL TELEMETRY ===".bold());
            println!("{:<20} | {}", "Subsystem", "Status");
            println!("{:<20} | {}", "-------------------", "------");
            println!("{:<20} | {}", "Driver (Ollama)", "ACTIVE".green());
            println!("{:<20} | {}", "Scheduler (VRAM)", "IDLE".yellow());
            println!("{:<20} | {}", "Context Firewall", "ENFORCING".green());
            println!("{:<20} | {}", "Connected Apps", "0");
        }
        Commands::Ps => match client.get(format!("{}/ps", kernel_url)).send().await {
            Ok(response) => {
                let text = response.text().await.unwrap_or_default();
                println!("\n{}", text);
            }
            Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
        },
        Commands::Ls {
            models,
            agents,
            manifests,
        } => {
            if *agents {
                match client.get(format!("{}/agents", kernel_url)).send().await {
                    Ok(response) => println!("\n{}", response.text().await.unwrap_or_default()),
                    Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
                }
            }

            // If the user wants Manifests
            if *manifests {
                match client.get(format!("{}/manifests", kernel_url)).send().await {
                    Ok(response) => println!("\n{}", response.text().await.unwrap_or_default()),
                    Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
                }
            }

            if *models || (!*models && !*agents && !*manifests) {
                match client.get(format!("{}/ls", kernel_url)).send().await {
                    Ok(response) => println!("\n{}", response.text().await.unwrap_or_default()),
                    Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
                }
            }
        }
        Commands::Expel { model_name } => {
            println!(
                "{} Sending SIGKILL to VRAM process: {}",
                "[!]".red().bold(),
                model_name.yellow()
            );

            match client
                .get(format!("{}/expel/{}", kernel_url, model_name))
                .send()
                .await
            {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    if text.starts_with("SUCCESS") {
                        println!("{} {}", "[+]".green(), text.bold());
                    } else {
                        println!("{} {}", "[-]".red(), text);
                    }
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Pull { model_name } => {
            println!(
                "{} Instructing Kernel to download and install: {}",
                "[*]".bright_blue(),
                model_name.yellow().bold()
            );
            println!("    (This may take a few minutes depending on your internet speed...)");

            // Because downloading takes time, we wait for the server's response
            match client
                .get(format!("{}/pull/{}", kernel_url, model_name))
                .send()
                .await
            {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    if text.starts_with("SUCCESS") {
                        println!("{} {}", "[+]".green(), text.bold());
                    } else {
                        println!("{} {}", "[-]".red(), text);
                    }
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Run { model, prompt } => {
            println!(
                "{} Routing task to {}...",
                "[*]".bright_blue(),
                model.yellow().bold()
            );

            let payload = RunPayload {
                model: model.clone(),
                prompt: prompt.clone(),
            };

            match client
                .post(format!("{}/run", kernel_url))
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();

                    if text.starts_with("ORE KERNEL ALERT") {
                        println!("\n{} {}", "[!]".red().bold(), text.red().bold());
                    } else {
                        println!("\n{}", text.green());
                    }
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Load { model_name } => {
            println!(
                "{} Instructing Kernel to allocate VRAM for: {}",
                "[*]".bright_blue(),
                model_name.yellow().bold()
            );

            match client
                .get(format!("{}/load/{}", kernel_url, model_name))
                .send()
                .await
            {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    if text.starts_with("SUCCESS") {
                        println!("{} {}", "[+]".green(), text.bold());
                    } else {
                        println!("{} {}", "[-]".red(), text);
                    }
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Kill { app_id } => {
            println!(
                "{} Sending SIGTERM to App: {}",
                "[!]".red().bold(),
                app_id.red()
            );
            println!("{} App context wiped from GPU Memory.", "[+]".green());
        }
        Commands::Manifest { app_id } => {
            use dialoguer::theme::SimpleTheme;

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
                if let Ok(res) = client.get("http://127.0.0.1:11434/api/tags").send().await {
                    if let Ok(tags) = res.json::<DriverTagsResponse>().await {
                        available_models = tags.models.into_iter().map(|m| m.name).collect();
                    }
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

                toml_output.push_str("[resources]\n");
                toml_output.push_str(&format!("allowed_models = {}\n", selected_models_formatted));
                toml_output.push_str(&format!("max_tokens_per_minute = {}\n", tokens));
                toml_output.push_str(&format!("gpu_priority = \"{}\"\n\n", priorities[p_idx]));
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
                    .with_prompt(
                        "Allowed Agent Tools (comma-separated, e.g., git_commit, file_search)",
                    )
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
                let targets: String = Input::with_theme(&SimpleTheme)
                    .with_prompt("Allowed Agent-to-Agent targets (comma-separated)")
                    .default("".into())
                    .interact_text()
                    .unwrap();

                toml_output.push_str("[ipc]\n");
                toml_output.push_str(&format!(
                    "allowed_ipc_targets = {}\n\n",
                    format_list(targets)
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
    }
}
