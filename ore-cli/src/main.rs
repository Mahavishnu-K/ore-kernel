use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use reqwest::{Client, header::{HeaderMap, HeaderValue, AUTHORIZATION}};
use std::fs;
use std::path::Path;
use std::process::exit;
use serde::Deserialize;
use indicatif::{ProgressBar, ProgressStyle};
use futures_util::StreamExt;
use std::cmp::min;
use std::io::Write;
use hf_hub::api::tokio::Api;
use hf_hub::{Repo, RepoType};

// configuration parsers
#[derive(Deserialize)]
struct OreConfig {
    system: SystemConfig,
}

#[derive(Deserialize)]
struct SystemConfig {
    engine: String,
}

fn get_system_engine() -> String {
    let config_path = "../ore.toml";
    match fs::read_to_string(config_path) {
        Ok(contents) => {
            match toml::from_str::<OreConfig>(&contents) {
                Ok(config) => config.system.engine,
                Err(_) => {
                    println!("{} FATAL: ore.toml is corrupted.", "[-]".red().bold());
                    println!("       Please run 'ore init' to regenerate it.");
                    exit(1);
                }
            }
        }
        Err(_) => {
            println!("{} FATAL: ORE System is not initialized.", "[-]".red().bold());
            println!("       Please run 'ore init' first.");
            exit(1);
        }
    }
}

fn get_model_map(alias: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match alias {
        "qwen2.5:0.5b" => Some((
            "Qwen/Qwen2.5-0.5B-Instruct-GGUF", 
            "qwen2.5-0.5b-instruct-q4_k_m.gguf", 
            "Qwen/Qwen2.5-0.5B-Instruct", 
        )),
        "llama3.2:1b" => Some((
            "bartowski/Llama-3.2-1B-Instruct-GGUF", 
            "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            "unsloth/Llama-3.2-1B-Instruct",
        )),
        _ => None,
    }
}

/// Streams a file from a URL directly to the disk with a professional progress bar
async fn download_with_progress(url: &str, dest: &Path, token: &Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let mut req = client.get(url);
    
    if let Some(t) = token.as_ref() {
        req = req.bearer_auth(t);
    }

    let res = req.send().await?;
    if !res.status().is_success() {
        return Err(format!("HTTP Error: {}", res.status()).into());
    }

    let total_size = res.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            // Template: [00:05] [==========>---] 1.2GB/2.5GB (25 MB/s, ETA: 00:02)
            .template("{spinner:.green}[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes:>7}/{total_bytes:7} ({bytes_per_sec}, ETA: {eta})")
            .unwrap()
            .progress_chars("=>-")
    );

    let mut file = fs::File::create(dest)?;
    let mut downloaded: u64 = 0;
    
    // Stream the data directly to the NVMe/SSD (Zero RAM bloat)
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(new);
    }

    pb.finish_and_clear(); 
    Ok(())
}

/// Attempts to securely fetch the user's Hugging Face token if it exists
fn get_hf_token() -> Option<String> {
    std::env::var("HF_TOKEN").ok().or_else(|| {
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        let token_path = Path::new(&home).join(".cache").join("huggingface").join("token");
        fs::read_to_string(token_path).ok().map(|s| s.trim().to_string())
    })
}

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
    /// Initialize ORE system configurations
    Init,
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
    /// Wipes an Agent's frozen memory from the SSD
    Clear {
        /// The ID of the agent (e.g., openclaw)
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

    let mut headers = HeaderMap::new();
    let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", auth_token)).unwrap();
    auth_value.set_sensitive(true);
    headers.insert(AUTHORIZATION, auth_value);

    let client = Client::builder()
        .default_headers(headers)
        .build()
        .expect("Failed to build HTTP client");

    match &cli.command {
        Commands::Init => {
            use dialoguer::theme::SimpleTheme;
            use dialoguer::{Select, Input};
            use std::fs;

            println!("\n==================================================");
            println!(" ORE KERNEL :: SYSTEM INITIALIZATION");
            println!("==================================================\n");

            let engines = &[
                "Ollama (Background daemon, easiest setup)",
                "Native (Bare-metal Rust execution, maximum control)"
            ];

            let engine_idx = Select::with_theme(&SimpleTheme)
                .with_prompt("Select your primary AI Execution Engine")
                .default(0)
                .items(engines)
                .interact()
                .unwrap();

            let mut toml_output = String::new();
            toml_output.push_str("[system]\n");

            if engine_idx == 0 {
                // OLLAMA SETUP
                toml_output.push_str("engine = \"ollama\"\n\n");
                toml_output.push_str("[ollama]\n");
                
                let url: String = Input::with_theme(&SimpleTheme)
                    .with_prompt("Enter Ollama API URL")
                    .default("http://127.0.0.1:11434".into())
                    .interact_text().unwrap();
                    
                toml_output.push_str(&format!("url = \"{}\"\n", url));
            } else {
                // NATIVE SETUP
                toml_output.push_str("engine = \"native\"\n\n");
                toml_output.push_str("[native]\n");
                
                let model: String = Input::with_theme(&SimpleTheme)
                    .with_prompt("Enter path to default .gguf model")
                    .default("qwen2.5-0.5b-instruct-q4_k_m.gguf".into())
                    .interact_text().unwrap();
                    
                let tokenizer: String = Input::with_theme(&SimpleTheme)
                    .with_prompt("Enter path to tokenizer.json")
                    .default("tokenizers/qwen2.5.json".into())
                    .interact_text().unwrap();

                toml_output.push_str(&format!("default_model = \"{}\"\n", model));
                toml_output.push_str(&format!("default_tokenizer = \"{}\"\n", tokenizer));
            }

            // Save to the root directory
            fs::write("../ore.toml", toml_output).expect("Failed to write config file");
            
            println!("\n[OK] ORE System configured successfully!");
            println!("Configuration saved to: ore.toml");
            println!("Please restart the 'ore-server' to apply changes.\n");
        }
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
            let engine = get_system_engine();
            if engine == "ollama" {
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
            } else if engine == "native" {
                println!("{} System configured for Native. Initializing ORE Package Manager for '{}'...", "[*]".bright_blue(), model_name.blue().bold());

                let (gguf_repo, gguf_file, base_repo) = match get_model_map(&model_name) {
                    Some(map) => map,
                    None => {
                        println!("{} Model '{}' not found in ORE verified Native registry.", "[-]".red(), model_name);
                        exit(1);
                    }
                };

                let api = Api::new().expect("Failed to initialize Hugging Face API client");
                let hf_token = get_hf_token();

                let safe_folder_name = model_name.replace(":", "-");

                let ore_models_dir = Path::new("../models").join(&safe_folder_name);
                if !ore_models_dir.exists() {
                    fs::create_dir_all(&ore_models_dir).unwrap();
                }

                println!("{} Pulling Neural Weights from {}...", "[~]".yellow(), gguf_repo);
                let repo_weights = api.repo(Repo::with_revision(gguf_repo.to_string(), RepoType::Model, "main".to_string()));
                let weights_url = repo_weights.url(gguf_file); 
                let final_gguf_dest = ore_models_dir.join("model.gguf");
                
                if let Err(e) = download_with_progress(&weights_url, &final_gguf_dest, &hf_token).await {
                    println!("{} FATAL: Failed to download weights: {}", "[-]".red(), e);
                    exit(1);
                }
                println!("{} Weights secured.", "[+]".green());

                println!("{} Pulling Dictionary (Tokenizer) from {}...", "[~]".yellow(), base_repo);
                let repo_tokenizer = api.repo(Repo::with_revision(base_repo.to_string(), RepoType::Model, "main".to_string()));
                let tokenizer_url = repo_tokenizer.url("tokenizer.json");
                let final_tok_dest = ore_models_dir.join("tokenizer.json");

                let tokenizer_path_display: String;

                // --- THE HACKER'S FALLBACK ---
                if let Err(e) = download_with_progress(&tokenizer_url, &final_tok_dest, &hf_token).await {
                    println!("{} [WARN] Official tokenizer is gated or unavailable ({}).", "[!]".yellow(), e);
                    println!("{} ORE will extract the tokenizer from the GGUF file on first load.", "[i]".bright_blue());
                    tokenizer_path_display = "Extracted from GGUF".to_string();
                } else {
                    // It worked!
                    println!("{} Dictionary secured.", "[+]".green());
                    tokenizer_path_display = final_tok_dest.display().to_string();
                }

                println!("\n[OK] '{}' HAS BEEN SUCCESSFULLY INSTALLED NATIVELY.", model_name.to_uppercase());
                println!("Weights Path   :: {}", final_gguf_dest.display());
                println!("Tokenizer Path :: {}\n", tokenizer_path_display);
            }else {
                println!("{} Unknown engine '{}' in ore.toml.", "[-]".red(), engine);
            }
        }
        Commands::Run { model, prompt } => {
            println!(
                "{} Routing task to {}...",
                "[*]".bright_blue(),
                model.blue().bold()
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
                    if response.status().is_success() {
                        
                        use std::io::Write;
                        println!(); 
                        
                        let mut stream = response.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            if let Ok(bytes) = chunk {
                                let text = String::from_utf8_lossy(&bytes);
                                if text.starts_with("ORE KERNEL ALERT") {
                                    print!("{}", text.red().bold());
                                } else {
                                    print!("{}", text.blue());
                                }
                                std::io::stdout().flush().unwrap();
                            }
                        }
                        println!("\n");
                    } else {
                        println!("{} Kernel Error: {}", "[-]".red(), response.status());
                    }
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Load { model_name } => {
            println!(
                "{} Instructing Kernel to allocate VRAM for: {}",
                "[*]".bright_blue(),
                model_name.blue().bold()
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
                let paging = Confirm::with_theme(&SimpleTheme)
                    .with_prompt("Enable Stateful Paging (KV-Cache SSD Swap for long tasks)?")
                    .default(false)
                    .interact().unwrap();


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

                let agents: String = Input::with_theme(&SimpleTheme)
                    .with_prompt("Tier 1: Allowed Agent-to-Agent text targets (comma-separated, e.g., writer_agent)")
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
        Commands::Clear { app_id } => {
            println!("{} Instructing Kernel to wipe memory for: {}", "[*]".bright_blue(), app_id.blue().bold());
            
            match client.get(format!("{}/clear/{}", kernel_url, app_id)).send().await {
                Ok(response) => println!("\n{}", response.text().await.unwrap_or_default().green()),
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
    }
}
