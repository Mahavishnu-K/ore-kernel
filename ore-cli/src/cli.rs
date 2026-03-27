use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ore")]
#[command(version = "0.1.0", about = "Control the ORE Kernel", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
