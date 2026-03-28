use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;
use std::{cmp::min, fs, io::Write, path::Path, process::exit};

#[derive(Deserialize)]
pub struct OreConfig {
    system: SystemConfig,
}

#[derive(Deserialize)]
pub struct SystemConfig {
    engine: String,
}

pub fn get_system_engine() -> String {
    let config_path = "../ore.toml";
    match fs::read_to_string(config_path) {
        Ok(contents) => match toml::from_str::<OreConfig>(&contents) {
            Ok(config) => config.system.engine,
            Err(_) => {
                println!("{} FATAL: ore.toml is corrupted.", "[-]".red().bold());
                println!("       Please run 'ore init' to regenerate it.");
                exit(1);
            }
        },
        Err(_) => {
            println!(
                "{} FATAL: ORE System is not initialized.",
                "[-]".red().bold()
            );
            println!("       Please run 'ore init' first.");
            exit(1);
        }
    }
}

pub enum ModelAsset {
    Gguf {
        gguf_repo: &'static str,
        gguf_file: &'static str,
        base_repo: &'static str,
    },
    Safetensors {
        repo: &'static str,
    },
}

/// Maps a simple user alias to Hugging Face repositories
pub fn get_model_map(alias: &str) -> Option<ModelAsset> {
    match alias {
        "qwen2.5:0.5b" => Some(ModelAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
            gguf_file: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-0.5B-Instruct",
        }),
        "qwen2.5:0.5b-base" => Some(ModelAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-0.5B-GGUF",
            gguf_file: "qwen2.5-0.5b-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-0.5B",
        }),
        "llama3.2:1b" => Some(ModelAsset::Gguf {
            gguf_repo: "bartowski/Llama-3.2-1B-Instruct-GGUF",
            gguf_file: "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            base_repo: "unsloth/Llama-3.2-1B-Instruct",
        }),
        "llama3.2:1b-base" => Some(ModelAsset::Gguf {
            gguf_repo: "bartowski/Llama-3.2-1B-GGUF",
            gguf_file: "Llama-3.2-1B-Q4_K_M.gguf",
            base_repo: "unsloth/Llama-3.2-1B",
        }),

        // --- SYSTEM EMBEDDERS (SAFETENSORS) ---
        "system-embedder" => Some(ModelAsset::Safetensors {
            repo: "nomic-ai/nomic-embed-text-v1.5",
        }),
        "all-minilm" => Some(ModelAsset::Safetensors {
            repo: "sentence-transformers/all-MiniLM-L6-v2",
        }),
        _ => None,
    }
}

/// Streams a file from a URL directly to the disk with a professional progress bar
pub async fn download_with_progress(
    url: &str,
    dest: &Path,
    token: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
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
pub fn get_hf_token() -> Option<String> {
    std::env::var("HF_TOKEN").ok().or_else(|| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let token_path = Path::new(&home)
            .join(".cache")
            .join("huggingface")
            .join("token");
        fs::read_to_string(token_path)
            .ok()
            .map(|s| s.trim().to_string())
    })
}

pub fn build_secure_client() -> Client {
    let token_path = "../ore-server/ore-kernel.token";
    let auth_token = match fs::read_to_string(token_path) {
        Ok(t) => t,
        Err(_) => {
            println!(
                "{} FATAL: Could not read Kernel Security Token.",
                "[-]".red().bold()
            );
            println!("    Is the ORE Kernel running? Did you start `ore-server`?");
            exit(1);
        }
    };

    let mut headers = HeaderMap::new();
    let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", auth_token)).unwrap();
    auth_value.set_sensitive(true);
    headers.insert(AUTHORIZATION, auth_value);

    Client::builder()
        .default_headers(headers)
        .build()
        .expect("Failed to build HTTP client")
}
