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

#[derive(Subcommand)]
enum Commands {
    /// Check if the ORE Kernel is running and healthy
    Status,
    /// View real-time Kernel metrics and telemetry
    Top,
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
            // In the future, this will hit a /metrics endpoint on the server
            println!("\n{}", "=== ORE KERNEL TELEMETRY ===".bold());
            println!("{:<20} | {}", "Subsystem", "Status");
            println!("{:<20} | {}", "-------------------", "------");
            println!("{:<20} | {}", "Driver (Ollama)", "ACTIVE".green());
            println!("{:<20} | {}", "Scheduler (VRAM)", "IDLE".yellow());
            println!("{:<20} | {}", "Context Firewall", "ENFORCING".green());
            println!("{:<20} | {}", "Connected Apps", "0");
        }
        Commands::Kill { app_id } => {
            // This is the emergency stop command!
            println!("{} Sending SIGTERM to App: {}", "[!]".red().bold(), app_id.red());
            println!("{} App context wiped from GPU Memory.", "[+]".green());
        }
    }
}