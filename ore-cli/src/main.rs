mod cli;
mod interactive;
mod utils;

use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use futures_util::StreamExt;
use hf_hub::{Repo, RepoType, api::tokio::Api};
use std::{fs, process::exit};
use utils::{
    OreAsset, build_secure_client, download_with_progress, get_asset_map, get_hf_token,
    get_system_engine,
};

#[derive(serde::Serialize)]
struct RunPayload {
    app_id: String,
    model: String,
    prompt: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let kernel_url = "http://127.0.0.1:6767";

    let client = if !matches!(cli.command, Commands::Init) {
        Some(build_secure_client())
    } else {
        None
    };

    match &cli.command {
        Commands::Init => {
            interactive::run_init_wizard();
        }
        Commands::Status => {
            println!("{} Pinging ORE Kernel...", "[*]".bright_blue());

            match client
                .unwrap()
                .get(format!("{}/health", kernel_url))
                .send()
                .await
            {
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
            match client
                .unwrap()
                .get(format!("{}/top", kernel_url))
                .send()
                .await
            {
                Ok(response) => println!("\n{}", response.text().await.unwrap_or_default()),
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Ps => match client
            .unwrap()
            .get(format!("{}/ps", kernel_url))
            .send()
            .await
        {
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
            let c = client.unwrap();
            if *agents {
                match c.get(format!("{}/agents", kernel_url)).send().await {
                    Ok(response) => println!("\n{}", response.text().await.unwrap_or_default()),
                    Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
                }
            }

            // If the user wants Manifests
            if *manifests {
                match c.get(format!("{}/manifests", kernel_url)).send().await {
                    Ok(response) => println!("\n{}", response.text().await.unwrap_or_default()),
                    Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
                }
            }

            if *models || (!*agents && !*manifests) {
                match c.get(format!("{}/ls", kernel_url)).send().await {
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
                .unwrap()
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
                    .unwrap()
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
                println!(
                    "{} System configured for Native. Initializing ORE Package Manager for '{}'...",
                    "[*]".bright_blue(),
                    model_name.blue().bold()
                );

                let asset_spec = match get_asset_map(model_name) {
                    Some(map) => map,
                    None => {
                        println!(
                            "{} Model '{}' not found in ORE verified Native registry.",
                            "[-]".red(),
                            model_name
                        );
                        exit(1);
                    }
                };

                let api = Api::new().expect("Failed to initialize Hugging Face API client");
                let hf_token = get_hf_token();

                let safe_folder_name = model_name.replace(":", "-");

                let base_dir = crate::utils::get_ore_dir();
                let ore_models_dir = base_dir.join("models").join(&safe_folder_name);

                if !ore_models_dir.exists() {
                    fs::create_dir_all(&ore_models_dir).unwrap();
                }

                match asset_spec {
                    OreAsset::Gguf {
                        gguf_repo,
                        gguf_file,
                        base_repo,
                    } => {
                        println!("{} Architecture: quantized GGUF", "[i]".cyan());
                        println!(
                            "{} Pulling Neural Weights from {}...",
                            "[~]".yellow(),
                            gguf_repo
                        );

                        let repo_weights = api.repo(Repo::with_revision(
                            gguf_repo.to_string(),
                            RepoType::Model,
                            "main".to_string(),
                        ));
                        let weights_url = repo_weights.url(gguf_file);
                        let final_gguf_dest = ore_models_dir.join("model.gguf");

                        if let Err(e) =
                            download_with_progress(&weights_url, &final_gguf_dest, &hf_token).await
                        {
                            println!("{} FATAL: Failed to download weights: {}", "[-]".red(), e);
                            exit(1);
                        }
                        println!("{} Weights secured.", "[+]".green());

                        println!(
                            "{} Pulling Dictionary (Tokenizer) from {}...",
                            "[~]".yellow(),
                            base_repo
                        );
                        let repo_tokenizer = api.repo(Repo::with_revision(
                            base_repo.to_string(),
                            RepoType::Model,
                            "main".to_string(),
                        ));
                        let tokenizer_url = repo_tokenizer.url("tokenizer.json");
                        let final_tok_dest = ore_models_dir.join("tokenizer.json");

                        let tokenizer_path_display: String;

                        if let Err(e) =
                            download_with_progress(&tokenizer_url, &final_tok_dest, &hf_token).await
                        {
                            println!(
                                "{} [WARN] Official tokenizer is gated or unavailable ({}).",
                                "[!]".yellow(),
                                e
                            );
                            println!(
                                "{} ORE will extract the tokenizer from the GGUF file on first load.",
                                "[i]".bright_blue()
                            );
                            tokenizer_path_display = "Extracted from GGUF".to_string();
                        } else {
                            println!("{} Dictionary secured.", "[+]".green());
                            tokenizer_path_display = final_tok_dest.display().to_string();
                        }

                        println!(
                            "\n{} '{}' INSTALLED NATIVELY.",
                            "[OK]".green(),
                            model_name.to_uppercase()
                        );
                        println!("Weights Path   :: {}", final_gguf_dest.display());
                        println!("Tokenizer Path :: {}\n", tokenizer_path_display);
                    }

                    OreAsset::Safetensors { repo } => {
                        println!(
                            "{} Architecture: Safetensors (Cloud Standard)",
                            "[i]".cyan()
                        );
                        let hf_repo = api.repo(Repo::with_revision(
                            repo.to_string(),
                            RepoType::Model,
                            "main".to_string(),
                        ));

                        println!("{} Pulling Safetensors from {}...", "[~]".yellow(), repo);
                        let st_url = hf_repo.url("model.safetensors");
                        let st_dest = ore_models_dir.join("model.safetensors");
                        if let Err(e) =
                            download_with_progress(&st_url, &st_dest, &hf_token.clone()).await
                        {
                            println!(
                                "{} FATAL: Failed to download safetensors: {}",
                                "[-]".red(),
                                e
                            );
                            exit(1);
                        }

                        // 2. Download Config
                        println!("{} Pulling config.json...", "[~]".yellow());
                        let config_url = hf_repo.url("config.json");
                        let config_dest = ore_models_dir.join("config.json");
                        download_with_progress(&config_url, &config_dest, &hf_token.clone())
                            .await
                            .unwrap();

                        // 3. Download Tokenizer
                        println!("{} Pulling tokenizer.json...", "[~]".yellow());
                        let tok_url = hf_repo.url("tokenizer.json");
                        let tok_dest = ore_models_dir.join("tokenizer.json");
                        download_with_progress(&tok_url, &tok_dest, &hf_token)
                            .await
                            .unwrap();

                        println!("{} All files secured.", "[+]".green());
                        println!(
                            "\n{} '{}' INSTALLED NATIVELY.",
                            "[OK]".green(),
                            model_name.to_uppercase()
                        );
                    }

                    OreAsset::Wasm {
                        url,
                        folder,
                        filename,
                    } => {
                        println!("{} Architecture: WebAssembly (WASM/WASI)", "[i]".cyan());

                        let target_dir = crate::utils::get_ore_dir().join(folder);
                        if !target_dir.exists() {
                            fs::create_dir_all(&target_dir).unwrap();
                        }

                        let final_dest = target_dir.join(filename);

                        println!(
                            "{} Pulling {} into {}/...",
                            "[~]".yellow(),
                            filename,
                            folder
                        );

                        // We use None for token since WASM runtimes are public GitHub releases
                        if let Err(e) = download_with_progress(url, &final_dest, &None).await {
                            println!(
                                "{} FATAL: Failed to download WASM runtime: {}",
                                "[-]".red(),
                                e
                            );
                            std::process::exit(1);
                        }

                        println!("{} Binary secured.", "[+]".green());
                        println!(
                            "\n{} '{}' INSTALLED NATIVELY.",
                            "[OK]".green(),
                            model_name.to_uppercase()
                        );
                        println!("Path :: {}\n", final_dest.display());
                    }
                }
            } else {
                println!("{} Unknown engine '{}' in ore.toml.", "[-]".red(), engine);
            }
        }
        Commands::Run { model, prompt } => {
            if let Some(p) = prompt {
                println!(
                    "{} Routing task to {}...",
                    "[*]".bright_blue(),
                    model.blue().bold()
                );

                let payload = RunPayload {
                    app_id: "terminal_user".to_string(),
                    model: model.clone(),
                    prompt: p.clone(),
                };

                let res = client
                    .unwrap()
                    .post(format!("{}/run", kernel_url))
                    .json(&payload)
                    .send()
                    .await
                    .unwrap();

                println!();
                let mut stream = res.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    if let Ok(bytes) = chunk {
                        let text = String::from_utf8_lossy(&bytes);
                        if text.starts_with("ORE KERNEL ALERT") {
                            print!("{}", text.red().bold());
                        } else {
                            print!("{}", text);
                        } // Standard terminal color for easy reading!
                        use std::io::Write;
                        std::io::stdout().flush().unwrap();
                    }
                }
                println!("\n");
            } else {
                println!(
                    "\n{}",
                    "╭──────────────────────────────────────────╮".bright_black()
                );
                println!(
                    "{}  ORE SESSION                             {}",
                    "│".bright_black(),
                    "│".bright_black()
                );
                println!(
                    "{}  Model: {:<32} {}",
                    "│".bright_black(),
                    model.yellow(),
                    "│".bright_black()
                );
                println!(
                    "{}  Type '/e' or '/exit' to disconnect      {}",
                    "│".bright_black(),
                    "│".bright_black()
                );
                println!(
                    "{}",
                    "╰──────────────────────────────────────────╯\n".bright_black()
                );

                let c = client.unwrap();

                let render_config = inquire::ui::RenderConfig::default()
                    .with_prompt_prefix(inquire::ui::Styled::new(""))
                    .with_answered_prompt_prefix(inquire::ui::Styled::new(""))
                    .with_text_input(inquire::ui::StyleSheet::new())
                    .with_answer(inquire::ui::StyleSheet::new());

                loop {
                    use std::io::{self, Write};

                    // --- USER TURN ---
                    let prompt_text = format!("{}", ">>>".bright_black().bold());
                    let input_result = inquire::Text::new(&prompt_text)
                        .with_placeholder(" Send a message...")
                        .with_render_config(render_config)
                        .prompt();

                    let trimmed = match input_result {
                        Ok(input) => input.trim().to_string(),
                        Err(_) => {
                            // This cleanly catches Ctrl+C or Escape keys!
                            println!("\n Session disconnected.");
                            break;
                        }
                    };

                    if trimmed == "/e" || trimmed == "/exit" {
                        println!("\n Session disconnected.");
                        break;
                    }

                    if trimmed.is_empty() {
                        // Move cursor back up if they just hit enter blindly
                        print!("\x1B[1A\x1B[2K");
                        continue;
                    }

                    let payload = RunPayload {
                        app_id: "terminal_user".to_string(),
                        model: model.clone(),
                        prompt: trimmed.to_string(),
                    };

                    match c
                        .post(format!("{}/run", kernel_url))
                        .json(&payload)
                        .send()
                        .await
                    {
                        Ok(response) => {
                            if response.status().is_success() {
                                let mut stream = response.bytes_stream();

                                let mut is_thinking = false;

                                while let Some(chunk) = stream.next().await {
                                    if let Ok(bytes) = chunk {
                                        let text = String::from_utf8_lossy(&bytes).to_string();
                                        if text.starts_with("ORE KERNEL ALERT") {
                                            print!("{}", text.red().bold());
                                            continue;
                                        }

                                        // Check for Thinking Tags - Thinking machine handling internal monologue vs final answer rendering
                                        if text.contains("<think>") {
                                            is_thinking = true;
                                            print!("{} ", "[Thinking...]".bright_black().italic());
                                            let clean = text.replace("<think>", "");
                                            print!("{}", clean.bright_black().italic());
                                            io::stdout().flush().unwrap();
                                            continue;
                                        }

                                        if text.contains("</think>") {
                                            is_thinking = false;
                                            let clean = text.replace("</think>", "");
                                            print!("{}", clean.bright_black().italic());
                                            print!("\n\n{} ", "[Answer]".blue().bold());
                                            io::stdout().flush().unwrap();
                                            continue;
                                        }

                                        // Render the text based on the current state
                                        if is_thinking {
                                            // Dim gray and italic for the internal monologue
                                            print!("{}", text.bright_black().italic());
                                        } else {
                                            // Bright blue for the final answer
                                            print!("{}", text.blue());
                                        }
                                        io::stdout().flush().unwrap();
                                    }
                                }
                                println!("\n");
                            } else {
                                println!("{} Kernel Error: {}", "[-]".red(), response.status());
                            }
                        }
                        Err(_) => {
                            println!("{} ORE Kernel is offline.", "[-]".red());
                            break;
                        }
                    }
                }
            }
        }
        Commands::Load { model_name } => {
            println!(
                "{} Instructing Kernel to allocate VRAM for: {}",
                "[*]".bright_blue(),
                model_name.blue().bold()
            );

            match client
                .unwrap()
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

            match client
                .unwrap()
                .get(format!("{}/kill/{}", kernel_url, app_id))
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
        Commands::Manifest { app_id } => {
            interactive::run_manifest_wizard(app_id, client.as_ref().unwrap()).await;
        }
        Commands::Compact { app_id } => {
            println!(
                "{} Instructing Kernel to compress memory for: {}",
                "[*]".bright_blue(),
                app_id.blue().bold()
            );
            println!("    (This will lock the GPU for a few seconds...)");

            match client
                .unwrap()
                .get(format!("{}/compact/{}", kernel_url, app_id))
                .send()
                .await
            {
                Ok(response) => println!("\n{}", response.text().await.unwrap_or_default().green()),
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
        Commands::Clear { app_id } => {
            println!(
                "{} Instructing Kernel to wipe memory for: {}",
                "[*]".bright_blue(),
                app_id.blue().bold()
            );

            match client
                .unwrap()
                .get(format!("{}/clear/{}", kernel_url, app_id))
                .send()
                .await
            {
                Ok(response) => println!("\n{}", response.text().await.unwrap_or_default().green()),
                Err(_) => println!("{} ORE Kernel is offline.", "[-]".red()),
            }
        }
    }
}
