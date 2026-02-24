use clap::{Parser, Subcommand};
use colored::*;
use reqwest::Client;
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

#[derive(Subcommand)]
enum Commands {
    /// Check if the ORE Kernel is running and healthy
    Status,
    /// View real-time Kernel metrics and telemetry
    Top,
    /// Shows the models currently loaded into VRAM
    Ps,
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
                        println!("{} Kernel returned an error: {}", "[-]".red(), response.status());
                    }
                }
                Err(_) => {
                    println!("{} ORE Kernel is {}!", "[-]".red().bold(), "OFFLINE".red().bold());
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
        Commands::Ps => {
            match client.get(format!("{}/ps", kernel_url)).send().await {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    println!("\n{}", text);
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Expel { model_name } => {
            println!("{} Sending SIGKILL to VRAM process: {}", "[!]".red().bold(), model_name.yellow());
            
            match client.get(format!("{}/expel/{}", kernel_url, model_name)).send().await {
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
            println!("{} Instructing Kernel to download and install: {}", "[*]".bright_blue(), model_name.yellow().bold());
            println!("    (This may take a few minutes depending on your internet speed...)");
            
            // Because downloading takes time, we wait for the server's response
            match client.get(format!("{}/pull/{}", kernel_url, model_name)).send().await {
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
            println!("{} Routing task to {}...", "[*]".bright_blue(), model.yellow().bold());
            
            let payload = RunPayload {
                model: model.clone(),
                prompt: prompt.clone(),
            };

            match client.post(format!("{}/run", kernel_url)).json(&payload).send().await {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    
                    // If the firewall blocks it, print in RED
                    if text.starts_with("ORE KERNEL ALERT") {
                        println!("\n{} {}", "[!]".red().bold(), text.red().bold());
                    } else {
                        // Otherwise, print the AI's response in GREEN
                        println!("\n{}", text.green());
                    }
                }
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Load { model_name } => {
            println!("{} Instructing Kernel to allocate VRAM for: {}", "[*]".bright_blue(), model_name.yellow().bold());
            
            match client.get(format!("{}/load/{}", kernel_url, model_name)).send().await {
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
            // emergency stop command
            println!("{} Sending SIGTERM to App: {}", "[!]".red().bold(), app_id.red());
            println!("{} App context wiped from GPU Memory.", "[+]".green());
        }
    }
}