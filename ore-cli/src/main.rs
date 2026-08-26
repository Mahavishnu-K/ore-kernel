mod cli;
mod interactive;
mod utils;

use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use futures_util::StreamExt;
use hf_hub::{Repo, RepoType, api::tokio::Api};
use std::path::{Path, PathBuf};
use std::{fs, process::exit};
use utils::{
    OreAsset, build_secure_client, download_with_progress, get_asset_map, get_hf_token,
    get_ore_dir, get_system_engine,
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
        Commands::MkTool {
            filepath,
            name,
            env,
            shared,
            host,
        } => {
            if *shared && *host {
                println!(
                    "{} FATAL: A module cannot be compiled as both a --shared Plugin and a --host Tool.",
                    "[-]".red().bold()
                );
                exit(1);
            }
            let path = Path::new(filepath);
            if !path.exists() {
                println!(
                    "{} FATAL: File '{}' not found.",
                    "[-]".red().bold(),
                    filepath
                );
                exit(1);
            }

            let tool_name = name
                .clone()
                .unwrap_or_else(|| path.file_stem().unwrap().to_str().unwrap().to_string());

            let (target_folder, extension) = if *shared {
                ("plugins", "wasi.so")
            } else {
                ("tools", "wasm")
            };

            let dest_dir = get_ore_dir().join(target_folder);
            if !dest_dir.exists() {
                fs::create_dir_all(&dest_dir).unwrap();
            }
            let absolute_dest_dir = fs::canonicalize(&dest_dir).unwrap();
            let dest_file = absolute_dest_dir.join(format!("{}.{}", tool_name, extension));
            let display_path = dest_file.display().to_string().replace("\\\\?\\", "");

            println!(
                "{} ORE Toolchain forging '{}' into a secure {}...",
                "[*]".bright_blue(),
                tool_name.cyan().bold(),
                if *shared {
                    "WASI Shared Object Plugin"
                } else {
                    "WASM Cartridge"
                }
            );

            if path.is_dir() {
                if path.join("package.json").exists()
                    || path.join("go.mod").exists()
                    || path.join("__main__.py").exists()
                {
                    // THE PROJECT SAFETY BLOCKER
                    if *shared {
                        println!(
                            "{} FATAL: Node.js, Go, and Python projects use Garbage Collectors.",
                            "[-]".red().bold()
                        );
                        println!(
                            "    They cannot be compiled into True Memory Fusion Plugins (.wasi.so)."
                        );
                        println!("    Please use Rust, C, C++, or Zig for Plugins.");
                        exit(1);
                    }

                    if *host {
                        println!(
                            "{} FATAL: Interpreted languages cannot act as C-ABI Hosts.",
                            "[-]".red().bold()
                        );
                        println!(
                            "    To use plugins in Python/JS, use the @ore/sdk instead of the --host flag."
                        );
                        exit(1);
                    }
                }

                if path.join("Cargo.toml").exists() {
                    // ------------------------- RUST PROJECT -------------------------
                    println!(
                        "{} Detected Project: {}",
                        "[i]".bright_black(),
                        "Rust (Cargo)".red().bold()
                    );

                    let cargo_str = fs::read_to_string(path.join("Cargo.toml")).unwrap();
                    let cargo_val: toml::Value = toml::from_str(&cargo_str).unwrap();

                    if *shared {
                        let mut is_cdylib = false;
                        if let Some(lib) = cargo_val.get("lib")
                            && let Some(crate_types) =
                                lib.get("crate-type").and_then(|v| v.as_array())
                            && crate_types.iter().any(|v| v.as_str() == Some("cdylib"))
                        {
                            is_cdylib = true;
                        }

                        if !is_cdylib {
                            println!(
                                "{} FATAL: Cargo.toml is missing the C-Dynamic Library declaration.",
                                "[-]".red().bold()
                            );
                            println!(
                                "    To compile a Rust project into a Memory Fusion Plugin (.wasi.so),"
                            );
                            println!(
                                "    you must explicitly tell Cargo to build a C-ABI library.\n"
                            );
                            println!(
                                "    {} Please add this exactly to your Cargo.toml:",
                                "[!]".yellow()
                            );
                            println!("\n    [lib]");
                            println!("    crate-type = [\"cdylib\"]\n");
                            exit(1);
                        }
                    }

                    let rust_target = if *shared {
                        "wasm32-unknown-unknown"
                    } else {
                        "wasm32-wasip1"
                    };

                    let _ = std::process::Command::new("rustup")
                        .args(["target", "add", rust_target])
                        .output();

                    println!(
                        "{} Building Cargo project (including all crates.io dependencies)...",
                        "[~]".yellow()
                    );

                    let mut cmd = std::process::Command::new("cargo");
                    cmd.current_dir(path);

                    if *shared || *host {
                        // FOR CUSTOM OS LINKING: We must use `rustc` to pass raw LLVM flags
                        cmd.args(["rustc", "--target", rust_target, "--release", "--"]);

                        if *shared {
                            cmd.args(["-C", "relocation-model=pic", "-C", "link-arg=-shared"]);
                        } else if *host {
                            cmd.args([
                                "-C",
                                "link-arg=--export-dynamic",
                                "-C",
                                "link-arg=--import-table",
                            ]);
                        }
                    } else {
                        cmd.args(["build", "--target", rust_target, "--release"]);
                    }

                    let build = cmd.output().expect("Failed to execute cargo build.");

                    if build.status.success() {
                        let cargo_str = fs::read_to_string(path.join("Cargo.toml")).unwrap();
                        let cargo_val: toml::Value = toml::from_str(&cargo_str).unwrap();
                        let pkg_name = cargo_val
                            .get("package")
                            .unwrap()
                            .get("name")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .replace("-", "_");

                        let compiled_wasm = path
                            .join("target")
                            .join(rust_target)
                            .join("release")
                            .join(format!("{}.wasm", pkg_name));
                        fs::copy(&compiled_wasm, &dest_file).expect("Failed to copy WASM");

                        println!("{} Cartridge forged successfully!", "[+]".green());
                        println!("Path :: {}", display_path.bright_black());
                    } else {
                        println!(
                            "{} Compilation failed:\n{}",
                            "[-]".red(),
                            String::from_utf8_lossy(&build.stderr)
                        );
                        exit(1);
                    }
                } else if path.join("package.json").exists() {
                    // ------------------------- NODE.JS PROJECT -------------------------
                    println!(
                        "{} Detected Project: {}",
                        "[i]".bright_black(),
                        "Node.js (NPM)".truecolor(247, 223, 30).bold()
                    );

                    let npx_cmd = if cfg!(target_os = "windows") {
                        "npx.cmd"
                    } else {
                        "npx"
                    };
                    let npm_cmd = if cfg!(target_os = "windows") {
                        "npm.cmd"
                    } else {
                        "npm"
                    };
                    let javy_cmd = if cfg!(target_os = "windows") {
                        "javy.cmd"
                    } else {
                        "javy"
                    };

                    if !path.join("node_modules").exists() {
                        println!("{} Installing NPM dependencies...", "[~]".yellow());
                        let npm_install = std::process::Command::new(npm_cmd)
                            .current_dir(path)
                            .args(["install"])
                            .output()
                            .unwrap();
                        if !npm_install.status.success() {
                            println!(
                                "{} NPM install failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&npm_install.stderr)
                            );
                            exit(1);
                        }
                    } else {
                        println!(
                            "{} Found 'node_modules'. Skipping install...",
                            "[i]".bright_black()
                        );
                    }

                    // Find the entry point automatically!
                    let entry_points = [
                        "index.ts",
                        "index.js",
                        "src/index.ts",
                        "src/index.js",
                        "main.ts",
                        "main.js",
                    ];
                    let mut entry_file = None;
                    for ep in entry_points {
                        if path.join(ep).exists() {
                            entry_file = Some(ep);
                            break;
                        }
                    }

                    if entry_file.is_none() {
                        println!(
                            "{} FATAL: Could not find entry point (index.ts, src/index.js, etc.)",
                            "[-]".red().bold()
                        );
                        exit(1);
                    }

                    println!("{} Bundling project via esbuild...", "[~]".yellow());
                    let tmp_js_dir = get_ore_dir().join(".tmp_build");
                    fs::create_dir_all(&tmp_js_dir).unwrap();

                    let absolute_tmp_dir = fs::canonicalize(&tmp_js_dir).unwrap();
                    let absolute_tmp_js = absolute_tmp_dir.join(format!("{}.js", tool_name));

                    let clean_tmp_js = absolute_tmp_js.to_str().unwrap().replace("\\\\?\\", "");
                    let clean_dest_file = dest_file.to_str().unwrap().replace("\\\\?\\", "");

                    let esbuild = std::process::Command::new(npx_cmd)
                        .current_dir(path)
                        .args([
                            "esbuild",
                            entry_file.unwrap(),
                            "--bundle",
                            "--format=esm",
                            &format!("--outfile={}", clean_tmp_js),
                        ])
                        .output()
                        .unwrap();

                    if !esbuild.status.success() {
                        println!(
                            "{} Bundling failed:\n{}",
                            "[-]".red(),
                            String::from_utf8_lossy(&esbuild.stderr)
                        );
                        exit(1);
                    }

                    if std::process::Command::new(javy_cmd)
                        .arg("--version")
                        .output()
                        .is_err()
                    {
                        std::process::Command::new(npm_cmd)
                            .args(["install", "-g", "javy-cli"])
                            .output()
                            .unwrap();
                    }

                    println!("{} Compiling bundled JS to WebAssembly...", "[~]".yellow());
                    let javy = std::process::Command::new(javy_cmd)
                        .args(["compile", &clean_tmp_js, "-o", &clean_dest_file])
                        .output()
                        .unwrap();

                    if javy.status.success() {
                        fs::remove_file(&absolute_tmp_js).unwrap();
                        println!("{} Cartridge forged successfully!", "[+]".green());
                        println!("Path :: {}", display_path.bright_black());
                    } else {
                        println!(
                            "{} Compilation failed:\n{}",
                            "[-]".red(),
                            String::from_utf8_lossy(&javy.stderr)
                        );
                        exit(1);
                    }
                } else if path.join("go.mod").exists() {
                    // ------------------------- GO PROJECT -------------------------
                    println!(
                        "{} Detected Project: {}",
                        "[i]".bright_black(),
                        "Go Modules".cyan().bold()
                    );

                    // auto-fetch dependencies if go.sum is missing
                    if !path.join("go.sum").exists() {
                        println!("{} Fetching Go dependencies...", "[~]".yellow());
                        let tidy = std::process::Command::new("go")
                            .current_dir(path)
                            .args(["mod", "tidy"])
                            .output()
                            .expect("Failed to execute go mod tidy. Is Go installed?");

                        if !tidy.status.success() {
                            println!(
                                "{} Failed to fetch Go dependencies:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&tidy.stderr)
                            );
                            exit(1);
                        }
                    } else {
                        println!(
                            "{} Found 'go.sum'. Skipping dependency fetch...",
                            "[i]".bright_black()
                        );
                    }

                    println!("{} Initializing Go WASI compiler...", "[~]".yellow());

                    let build = std::process::Command::new("go")
                        .current_dir(path)
                        .env("GOOS", "wasip1")
                        .env("GOARCH", "wasm")
                        .args(["build", "-o", dest_file.to_str().unwrap(), "."])
                        .output()
                        .expect("Failed to execute Go compiler.");

                    if build.status.success() {
                        println!("{} Cartridge forged successfully!", "[+]".green());
                        println!("Path :: {}", display_path.bright_black());
                    } else {
                        println!(
                            "{} Compilation failed:\n{}",
                            "[-]".red(),
                            String::from_utf8_lossy(&build.stderr)
                        );
                        exit(1);
                    }
                } else if path.join("__main__.py").exists() {
                    // ------------------------- PYTHON PROJECT -------------------------
                    println!(
                        "{} Detected Project: {}",
                        "[i]".bright_black(),
                        "Python Directory".yellow().bold()
                    );

                    if env != "data" {
                        println!(
                            "{} FATAL: Multi-file Python projects currently require the '--env data' flag.",
                            "[-]".red().bold()
                        );
                        println!("    Please run: ore mktool {} --env data", filepath);
                        exit(1);
                    }

                    let base_wasm_path = get_ore_dir().join("runtimes").join("system-py-data.wasm");
                    if !base_wasm_path.exists() {
                        println!(
                            "{} Pulling 'system-py-data' wasm tool via ore-cli...",
                            "[*]".bright_blue()
                        );
                        let current_exe =
                            std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ore"));
                        let install = std::process::Command::new(current_exe)
                            .args(["pull", "system-py-data"])
                            .output()
                            .unwrap();
                        if !install.status.success() {
                            println!("{} Failed to auto-install system-py-data.", "[-]".red());
                            exit(1);
                        }
                    }

                    // auto-vendor dependencies into a temporary directory
                    let vendor_dir = get_ore_dir()
                        .join(".tmp_build")
                        .join(&tool_name)
                        .join("vendor");
                    if vendor_dir.exists() {
                        fs::remove_dir_all(&vendor_dir).unwrap();
                    }

                    if path.join("requirements.txt").exists() {
                        println!(
                            "{} Found requirements.txt! Auto-vendoring dependencies...",
                            "[~]".yellow()
                        );

                        let pip_install = std::process::Command::new("pip")
                            .args([
                                "install",
                                "-r",
                                path.join("requirements.txt").to_str().unwrap(),
                                "--target",
                                vendor_dir.to_str().unwrap(),
                                "--upgrade", // Ensure fresh install
                            ])
                            .output()
                            .expect("Failed to execute pip.");

                        if !pip_install.status.success() {
                            println!(
                                "{} pip install failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&pip_install.stderr)
                            );
                            exit(1);
                        }

                        // The C-extension scanner (Fail-Fast Security)
                        println!(
                            "{} Scanning dependencies for WASM compatibility...",
                            "[~]".yellow()
                        );
                        for entry in walkdir::WalkDir::new(&vendor_dir) {
                            let entry = entry.unwrap();
                            if entry.path().is_file() {
                                let ext = entry
                                    .path()
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("");
                                if ["so", "pyd", "dylib", "dll"].contains(&ext) {
                                    println!(
                                        "{} FATAL: C-Extension detected in dependencies!",
                                        "[-]".red().bold()
                                    );
                                    println!("    File: {}", entry.path().display());
                                    println!(
                                        "    WebAssembly (WASI) strictly requires Pure Python packages."
                                    );
                                    println!(
                                        "    Please remove this package from requirements.txt."
                                    );
                                    fs::remove_dir_all(&vendor_dir).unwrap();
                                    exit(1);
                                }
                            }
                        }
                        println!("{} Dependencies verified as Pure Python.", "[+]".green());
                    }

                    println!(
                        "{} Zipping Python project into Fat Cartridge...",
                        "[~]".yellow()
                    );
                    let mut zip_buf = std::io::Cursor::new(Vec::new());
                    {
                        let mut zip = zip::ZipWriter::new(&mut zip_buf);
                        let options = zip::write::SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Stored);

                        // Use WalkDir to Recursively grab all Python files in the directory to the ZIP
                        for entry in walkdir::WalkDir::new(path) {
                            let entry = entry.unwrap();
                            let p = entry.path();

                            if p.is_file() {
                                // Get the relative path (so "utils/math.py" stays "utils/math.py" inside the zip)
                                let relative_path = p.strip_prefix(path).unwrap().to_str().unwrap();

                                // Convert Windows backslashes to Unix forward slashes for the Zip internal structure
                                let zip_path = relative_path.replace("\\", "/");

                                zip.start_file(zip_path, options).unwrap();
                                let contents = fs::read(p).unwrap();
                                use std::io::Write;
                                zip.write_all(&contents).unwrap();
                            }
                        }
                        zip.finish().unwrap();
                    }

                    let mut final_wasm = fs::read(&base_wasm_path).unwrap();
                    final_wasm.extend(zip_buf.into_inner());
                    fs::write(&dest_file, final_wasm).expect("Failed to write cartridge");

                    println!(
                        "{} Python Project frozen into WASM successfully!",
                        "[+]".green()
                    );
                    println!("Path :: {}", display_path.bright_black());
                } else if path.join("build.zig").exists() {
                    // ------------------------- ZIG PROJECT -------------------------
                    println!(
                        "{} Detected Project: {}",
                        "[i]".bright_black(),
                        "Zig (build.zig)".truecolor(247, 164, 29).bold()
                    );
                    println!("{} Initializing Zig WASI compiler...", "[~]".yellow());

                    let build = std::process::Command::new("zig")
                        .current_dir(path)
                        .args(["build", "-Dtarget=wasm32-wasi", "-Doptimize=ReleaseFast"])
                        .output()
                        .expect("Failed to execute zig build. Is Zig installed?");

                    if build.status.success() {
                        // Zig outputs built binaries to zig-out/bin/
                        let zig_out_bin = path.join("zig-out").join("bin");
                        let mut compiled_wasm = None;

                        // Intelligently scan the output directory for the compiled .wasm file
                        if zig_out_bin.exists() {
                            for entry in fs::read_dir(&zig_out_bin).unwrap() {
                                let entry = entry.unwrap();
                                let p = entry.path();
                                if p.is_file()
                                    && p.extension().and_then(|s| s.to_str()) == Some("wasm")
                                {
                                    compiled_wasm = Some(p);
                                    break; // Grab the first .wasm file we find!
                                }
                            }
                        }

                        if let Some(wasm_file) = compiled_wasm {
                            fs::copy(&wasm_file, &dest_file)
                                .expect("Failed to copy compiled WASM to tools folder");

                            println!("{} Cartridge forged successfully!", "[+]".green());
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            println!(
                                "{} FATAL: Zig compilation succeeded, but no .wasm file was found in 'zig-out/bin/'.",
                                "[-]".red().bold()
                            );
                            exit(1);
                        }
                    } else {
                        println!(
                            "{} Compilation failed:\n{}",
                            "[-]".red(),
                            String::from_utf8_lossy(&build.stderr)
                        );
                        exit(1);
                    }
                } else {
                    println!("{} FATAL: Unrecognized project type.", "[-]".red().bold());
                    println!(
                        "    Could not find Cargo.toml, package.json, go.mod, build.zig, or __main__.py in the directory."
                    );
                    exit(1);
                }
            } else {
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

                if (*shared || *host) && ["py", "js", "ts", "go"].contains(&ext) {
                    println!(
                        "{} FATAL: Language '{}' uses a Garbage Collector and cannot be compiled into a True Memory Fusion Plugin (.wasi.so).",
                        "[-]".red().bold(),
                        ext
                    );
                    println!("    Please use Rust, C, C++, or Zig for Plugins.");
                    exit(1);
                }

                match ext {
                    // Rust tools build
                    "rs" => {
                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            "Rust".red().bold()
                        );
                        let rust_target = if *shared {
                            "wasm32-unknown-unknown"
                        } else {
                            "wasm32-wasip1"
                        };
                        println!("{} Initializing Cargo WASI compiler...", "[~]".yellow());

                        // Check if the wasm32-wasip1 target is installed
                        let _ = std::process::Command::new("rustup")
                            .args(["target", "add", rust_target])
                            .output()
                            .expect("Failed to execute rustup. Is Rust installed?");

                        let build_dir = get_ore_dir().join(".tmp_build").join(&tool_name);

                        let mut cmd;

                        if *shared || *host {
                            if build_dir.exists() {
                                fs::remove_dir_all(&build_dir).unwrap();
                            }
                            fs::create_dir_all(build_dir.join("src")).unwrap();

                            let src_file = if *shared { "lib.rs" } else { "main.rs" };
                            fs::copy(filepath, build_dir.join("src").join(src_file)).unwrap();

                            let features = if *shared {
                                r#"features = ["plugin"]"#
                            } else if *host {
                                r#"features = ["host"]"#
                            } else {
                                ""
                            };

                            // Smart local resolution: If we are testing inside the repo, use the local path.
                            // Otherwise, fallback to pulling from crates.io (version = "*")
                            // Helper to strip Windows UNC paths and fix slashes for Cargo.toml
                            let clean_path = |p: std::path::PathBuf| -> String {
                                p.display()
                                    .to_string()
                                    .replace("\\\\?\\", "")
                                    .replace("\\", "/")
                            };

                            // Smart local resolution: Search common dev paths relative to CWD
                            let local_sys = std::fs::canonicalize("../ore-sys")
                                .ok()
                                .or_else(|| std::fs::canonicalize("ore-sys").ok())
                                .or_else(|| std::fs::canonicalize("../../ore-sys").ok());

                            let dep_line = if let Some(abs_path) = local_sys {
                                format!(
                                    r#"ore-sys = {{ path = "{}", {} }}"#,
                                    clean_path(abs_path),
                                    features
                                )
                            } else {
                                // Production Fallback: Pull from crates.io
                                format!(r#"ore-sys = {{ version = "*", {} }}"#, features)
                            };

                            let mut cargo_toml = format!(
                                r#"
    [workspace]
    # Isolates this build from the host's Cargo workspace!

    [package]
    name = "ore_tool"
    version = "0.1.0"
    edition = "2024"

    [dependencies]
    {}

    [profile.release]
    opt-level = 3
    lto = true
    codegen-units = 1
    strip = true
    "#,
                                dep_line
                            );

                            if *shared {
                                cargo_toml.push_str("\n[lib]\ncrate-type = [\"cdylib\"]\n");
                            }

                            fs::write(build_dir.join("Cargo.toml"), cargo_toml).unwrap();

                            cmd = std::process::Command::new("cargo");

                            cmd.current_dir(&build_dir);
                            // Compile using rustc directly to a standalone WASM file
                            cmd.args(["rustc", "--target", rust_target, "--release", "--"]);

                            if *shared {
                                cmd.args(["-C", "relocation-model=pic", "-C", "link-arg=-shared"]);
                            } else if *host {
                                cmd.args([
                                    "-C",
                                    "link-arg=--export-dynamic",
                                    "-C",
                                    "link-arg=--import-table",
                                ]);
                            }
                        } else {
                            cmd = std::process::Command::new("rustc");
                            cmd.args([
                                filepath,
                                "--target",
                                rust_target,
                                "-C",
                                "opt-level=3",
                                "-o",
                                dest_file.to_str().unwrap(),
                            ]);
                        }

                        let build = cmd.output().expect("Failed to execute compiler.");

                        if build.status.success() {
                            if *shared || *host {
                                let compiled_wasm = build_dir
                                    .join("target")
                                    .join(rust_target)
                                    .join("release")
                                    .join("ore_tool.wasm");
                                fs::copy(&compiled_wasm, &dest_file)
                                    .expect("Failed to copy compiled WASM to tools folder");

                                fs::remove_dir_all(&build_dir).unwrap();
                            }

                            println!("{} Cartridge forged successfully!", "[+]".green());
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            println!(
                                "{} Compilation failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&build.stderr)
                            );
                            exit(1);
                        }
                    }
                    // Go tools build
                    "go" => {
                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            "Go".cyan().bold()
                        );
                        println!(
                            "{} Initializing TinyGo / Go WASI compiler...",
                            "[~]".yellow()
                        );

                        let build = std::process::Command::new("go")
                            .env("GOOS", "wasip1")
                            .env("GOARCH", "wasm")
                            .args(["build", "-o", dest_file.to_str().unwrap(), filepath])
                            .output()
                            .expect("Failed to execute Go compiler. Is Go installed?");

                        if build.status.success() {
                            println!("{} Cartridge forged successfully!", "[+]".green());
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            println!(
                                "{} Compilation failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&build.stderr)
                            );
                        }
                    }
                    // Python tools build
                    "py" => {
                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            "Python".yellow().bold()
                        );

                        if env == "data" {
                            // MODE: Statically Linked CPython + Pandas/Numpy/Sci-kit Learn via Zip Append
                            println!(
                                "{} Environment: Data Science (Numpy/Pandas/Sci-kit Learn enabled)",
                                "[*]".bright_blue()
                            );
                            println!(
                                "{} Initiating VFS Zip-Append Compilation...",
                                "[~]".yellow()
                            );

                            let base_wasm_path =
                                get_ore_dir().join("runtimes").join("system-py-data.wasm");

                            if !base_wasm_path.exists() {
                                println!(
                                    "{} [WARN]: Data Science base engine not found.",
                                    "[!]".yellow()
                                );
                                println!(
                                    "{} Pulling 'system-py-data' wasm tool via ore-cli...",
                                    "[*]".bright_blue()
                                );
                                // DYNAMIC SELF-INVOCATION: Automatically runs itself (ore-cli)
                                let current_exe = std::env::current_exe()
                                    .unwrap_or_else(|_| PathBuf::from("ore"));

                                let install = std::process::Command::new(current_exe)
                                    .args(["pull", "system-py-data"])
                                    .output()
                                    .expect("Failed to run ore pull command.");

                                if !install.status.success() {
                                    println!(
                                        "{} Failed to auto-install system-py-data. Try running: ore pull system-py-data",
                                        "[-]".red()
                                    );
                                    exit(1);
                                }
                            }

                            let py_code =
                                fs::read_to_string(filepath).expect("Failed to read Python file");

                            // Create a Zip Archive in memory
                            let mut zip_buf = std::io::Cursor::new(Vec::new());
                            {
                                let mut zip = zip::ZipWriter::new(&mut zip_buf);
                                let options = zip::write::SimpleFileOptions::default()
                                    .compression_method(zip::CompressionMethod::Stored); // Uncompressed for speed

                                // Python WASI automatically executes __main__.py when appended!
                                zip.start_file("__main__.py", options).unwrap();
                                use std::io::Write;
                                zip.write_all(py_code.as_bytes()).unwrap();
                                zip.finish().unwrap();
                            }

                            // Read the Fat CPython WASM binary
                            let mut final_wasm = fs::read(&base_wasm_path).unwrap_or_else(|e| {
                                    println!(
                                        "{} FATAL: Data Science base engine is missing or corrupted: {}",
                                        "[-]".red().bold(),
                                        e
                                    );
                                    println!(
                                        "    Run 'ore pull system-py-data' to restore the heavy runtime."
                                    );
                                    exit(1);
                                });

                            // Concatenate the Zip bytes to the absolute end of the WASM binary!
                            final_wasm.extend(zip_buf.into_inner());

                            // Save the new, unified Cartridge
                            fs::write(&dest_file, final_wasm)
                                .expect("Failed to forge heavy WASM cartridge");

                            println!(
                                "{} Python Data Tool frozen into WASM successfully!",
                                "[+]".green()
                            );
                            println!(
                                "{} Note: Cartridge size is ~110MB. It contains the C-Extensions.",
                                "[i]".bright_black()
                            );
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            // MODE: Pure Python via RustPython AOT Compilation
                            println!(
                                "{} Initiating RustPython AOT Compilation...",
                                "[~]".yellow()
                            );
                            println!(
                                "{} Note: The first compilation will take 1-2 minutes to build the Python engine.",
                                "[i]".bright_black()
                            );
                            println!(
                                "{} Note: Resulting cartridge will be ~25MB. Pure Python only.",
                                "[i]".bright_black()
                            );

                            let _ = std::process::Command::new("rustup")
                                .args(["target", "add", "wasm32-wasip1"])
                                .output()
                                .expect("Failed to execute rustup. Is Rust installed?");

                            let build_dir = get_ore_dir().join(".tmp_build").join(&tool_name);
                            if build_dir.exists() {
                                fs::remove_dir_all(&build_dir).unwrap();
                            }
                            fs::create_dir_all(build_dir.join("src")).unwrap();

                            // Copy the developer's python script
                            let build_filename = tool_name.clone();
                            let py_code =
                                fs::read_to_string(filepath).expect("Failed to read Python file");
                            fs::write(build_dir.join("src").join(build_filename), py_code).unwrap();

                            // Write the Cargo.toml for the RustPython wrapper
                            let cargo_toml = r#"
[workspace]
# Empty workspace table isolates this build from the host's Cargo workspace!

[package]
name = "ore_tool"
version = "0.1.0"
edition = "2024"

[dependencies]
rustpython = "0.5.0"

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
"#;
                            fs::write(build_dir.join("Cargo.toml"), cargo_toml).unwrap();

                            // Write the Rust Execution Wrapper (RustPython 0.5.0 API)
                            let main_rs = format!(
                                r#"
fn main() {{
    use rustpython::{{InterpreterBuilder, InterpreterBuilderExt, vm}};

    // The official 0.5.0 builder automatically wires up the Standard Library for us!
    let interp = InterpreterBuilder::new()
        .init_stdlib()
        .build();

    interp.enter(|vm| {{
        let scope = vm.new_scope_with_builtins();
        
        // Bake the developer's exact script directly into the binary!
        let script = include_str!("{}");
        
        // Compile the script text into Python Bytecode
        let code_obj = vm.compile(script, vm::compiler::Mode::Exec, "<embedded>".to_owned())
            .expect("Failed to compile Python syntax");
        
        // Run the script!
        if let Err(e) = vm.run_code_obj(code_obj, scope) {{
            vm.print_exception(e);
            std::process::exit(1);
        }}
    }});
}}
"#,
                                tool_name
                            );
                            fs::write(build_dir.join("src").join("main.rs"), main_rs).unwrap();

                            // Compile the wrapped Python script to WASM!
                            let build = std::process::Command::new("cargo")
                                .current_dir(&build_dir)
                                .args(["build", "--target", "wasm32-wasip1", "--release"])
                                .output()
                                .expect("Failed to execute cargo build.");

                            if build.status.success() {
                                let compiled_wasm = build_dir
                                    .join("target")
                                    .join("wasm32-wasip1")
                                    .join("release")
                                    .join("ore_tool.wasm");
                                fs::copy(&compiled_wasm, &dest_file)
                                    .expect("Failed to copy compiled WASM to tools folder");
                                fs::remove_dir_all(&build_dir).unwrap(); // Clean up the temp build folder

                                println!("{} Python frozen into WASM successfully!", "[+]".green());
                                println!("Path :: {}", display_path.bright_black());
                            } else {
                                println!(
                                    "{} Compilation failed:\n{}",
                                    "[-]".red(),
                                    String::from_utf8_lossy(&build.stderr)
                                );
                                exit(1);
                            }
                        }
                    }
                    // JavaScript tools build
                    "js" => {
                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            "JavaScript".truecolor(247, 223, 30).bold()
                        );
                        println!("{} Initializing Javy compiler...", "[~]".yellow());

                        let npm_cmd = if cfg!(target_os = "windows") {
                            "npm.cmd"
                        } else {
                            "npm"
                        };
                        let javy_cmd = if cfg!(target_os = "windows") {
                            "javy.cmd"
                        } else {
                            "javy"
                        };

                        // AUTO-INSTALLER: Check if Javy is installed
                        if std::process::Command::new(javy_cmd)
                            .arg("--version")
                            .output()
                            .is_err()
                        {
                            println!(
                                "{} 'javy' compiler not found. Auto-installing via npm...",
                                "[*]".bright_blue()
                            );
                            let install = std::process::Command::new(npm_cmd)
                                .args(["install", "-g", "javy-cli"])
                                .output()
                                .expect("Failed to run npm. Is Node.js installed?");

                            if !install.status.success() {
                                println!(
                                    "{} Failed to auto-install javy. Try running: npm install -g javy-cli",
                                    "[-]".red()
                                );
                                exit(1);
                            }
                            println!("{} Javy installed successfully!", "[+]".green());
                        }

                        // Javy takes a JS file and fuses it with QuickJS into a single WASM binary.
                        let build = std::process::Command::new(javy_cmd)
                            .args(["compile", filepath, "-o", dest_file.to_str().unwrap()])
                            .output()
                            .expect("Failed to execute javy. Ensure Javy is installed by running: npm install -g javy-cli");

                        if build.status.success() {
                            println!("{} Cartridge forged successfully!", "[+]".green());
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            println!(
                                "{} Compilation failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&build.stderr)
                            );
                            exit(1);
                        }
                    }
                    // Typescript tools build
                    "ts" => {
                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            "TypeScript".blue().bold()
                        );
                        println!(
                            "{} Transpiling to JavaScript via esbuild...",
                            "[~]".yellow()
                        );

                        let npx_cmd = if cfg!(target_os = "windows") {
                            "npx.cmd"
                        } else {
                            "npx"
                        };
                        let npm_cmd = if cfg!(target_os = "windows") {
                            "npm.cmd"
                        } else {
                            "npm"
                        };
                        let javy_cmd = if cfg!(target_os = "windows") {
                            "javy.cmd"
                        } else {
                            "javy"
                        };

                        let build_dir = get_ore_dir().join(".tmp_build").join(&tool_name);
                        if build_dir.exists() {
                            fs::remove_dir_all(&build_dir).unwrap();
                        }
                        fs::create_dir_all(&build_dir).unwrap();
                        let js_out = build_dir.join(format!("{}.js", tool_name));

                        // Use npx esbuild to bundle TS into a single clean JS file
                        let tsc_build = std::process::Command::new(npx_cmd)
                            .args([
                                "esbuild",
                                filepath,
                                "--bundle",
                                "--format=esm",
                                &format!("--outfile={}", js_out.to_str().unwrap()),
                            ])
                            .output()
                            .expect(
                                "Failed to execute npx esbuild. Ensure Node.js and npx are installed.",
                            );

                        if !tsc_build.status.success() {
                            println!(
                                "{} TypeScript transpilation failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&tsc_build.stderr)
                            );
                            exit(1);
                        }

                        println!("{} Initializing Javy compiler...", "[~]".yellow());

                        // AUTO-INSTALLER: Check if Javy is installed
                        if std::process::Command::new(javy_cmd)
                            .arg("--version")
                            .output()
                            .is_err()
                        {
                            println!(
                                "{} 'javy' compiler not found. Auto-installing via npm...",
                                "[*]".bright_blue()
                            );
                            let install = std::process::Command::new(npm_cmd)
                                .args(["install", "-g", "javy-cli"])
                                .output()
                                .expect("Failed to run npm.");

                            if !install.status.success() {
                                println!(
                                    "{} Failed to auto-install javy. Try running: npm install -g javy-cli",
                                    "[-]".red()
                                );
                                exit(1);
                            }
                        }

                        let build = std::process::Command::new(javy_cmd)
                            .args(["compile", js_out.to_str().unwrap(), "-o", dest_file.to_str().unwrap()])
                            .output()
                            .expect("Failed to execute javy. Ensure Javy is installed by running: npm install -g javy-cli");

                        if build.status.success() {
                            fs::remove_dir_all(&build_dir).unwrap(); // Cleanup temp JS
                            println!("{} Cartridge forged successfully!", "[+]".green());
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            println!(
                                "{} Compilation failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&build.stderr)
                            );
                            exit(1);
                        }
                    }
                    // Zig tools build
                    "zig" => {
                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            "Zig".truecolor(247, 164, 29).bold()
                        );
                        println!("{} Initializing Zig WASI compiler...", "[~]".yellow());

                        let tmp_zig_dir = get_ore_dir().join(".tmp_build").join("zig_inject");
                        if *host {
                            fs::create_dir_all(&tmp_zig_dir).unwrap();
                            let sys_zig_content = include_str!("syskit/ore.zig");
                            fs::write(tmp_zig_dir.join("ore.zig"), sys_zig_content)
                                .expect("Failed to write temporary ore.zig SDK");
                            println!("{} Temporary Zig SDK written.", "[+]".green());
                        }

                        let mut cmd = std::process::Command::new("zig");

                        if *shared {
                            cmd.args([
                                "build-lib",
                                filepath,
                                "-target",
                                "wasm32-freestanding",
                                "-dynamic",
                                "-O",
                                "ReleaseFast",
                                &format!("-femit-bin={}", dest_file.to_str().unwrap()),
                            ]);
                        } else if *host {
                            let mod_path = tmp_zig_dir.join("ore.zig");
                            cmd.args([
                                "build-exe",
                                "-target",
                                "wasm32-wasi",
                                "-O",
                                "ReleaseFast",
                                "-rdynamic",
                                "--import-symbols",
                                "--import-table",
                                &format!("-femit-bin={}", dest_file.to_str().unwrap()),
                                "--dep",
                                "ore_sys",
                                &format!("-Mroot={}", filepath),
                                &format!("-More_sys={}", mod_path.display()),
                            ]);
                        } else {
                            cmd.args([
                                "build-exe",
                                filepath,
                                "-target",
                                "wasm32-wasi",
                                "-O",
                                "ReleaseFast",
                                &format!("-femit-bin={}", dest_file.to_str().unwrap()),
                            ]);
                        }

                        let build = cmd.output().expect("Failed to execute zig. Ensure Zig is installed by running: zig version");

                        if *host {
                            let _ = fs::remove_dir_all(&tmp_zig_dir);
                        }

                        if build.status.success() {
                            println!("{} Cartridge forged successfully!", "[+]".green());
                            println!("Path :: {}", display_path.bright_black());
                        } else {
                            println!(
                                "{} Compilation failed:\n{}",
                                "[-]".red(),
                                String::from_utf8_lossy(&build.stderr)
                            );
                            exit(1);
                        }
                    }
                    // C/C++ tools build
                    "c" | "cpp" | "cc" | "cxx" => {
                        let is_cpp = ext != "c";
                        let lang_name = if is_cpp { "C++" } else { "C" };
                        let compiler = if is_cpp { "c++" } else { "cc" };

                        println!(
                            "{} Detected Language: {}",
                            "[i]".bright_black(),
                            lang_name.magenta().bold()
                        );
                        println!(
                            "{} Initializing WASI-SDK compiler ({})...",
                            "[~]".yellow(),
                            compiler
                        );

                        let mut cmd = std::process::Command::new("zig");
                        cmd.arg(compiler).arg(filepath);

                        let tmp_include_dir = get_ore_dir().join(".tmp_build").join("cpp_inject");
                        if *shared || *host {
                            fs::create_dir_all(&tmp_include_dir).unwrap();
                            let ore_h_content = include_str!("syskit/ore.h");
                            fs::write(tmp_include_dir.join("ore.h"), ore_h_content)
                                .expect("Failed to write temporary ore.h SDK");

                            let include_arg = format!("-I{}", tmp_include_dir.display());
                            cmd.arg(&include_arg);
                        }

                        if *shared {
                            cmd.args([
                                "-target",
                                "wasm32-freestanding",
                                "-nostdlib",
                                "-shared",
                                "-fPIC",
                                "-DORE_PLUGIN_MODE",
                                "-Wl,--no-entry",
                                "-O3",
                                "-fno-sanitize=undefined",
                                "-o",
                                dest_file.to_str().unwrap(),
                            ]);
                        } else if *host {
                            cmd.args([
                                "-target",
                                "wasm32-wasi",
                                "-Wl,--export-dynamic",
                                "-Wl,--import-table",
                                "-O3",
                                "-o",
                                dest_file.to_str().unwrap(),
                            ]);
                        } else {
                            cmd.args([
                                "-target",
                                "wasm32-wasi",
                                "-O3",
                                "-o",
                                dest_file.to_str().unwrap(),
                            ]);
                        }

                        let build_result = cmd.output();

                        if *shared || *host {
                            let _ = fs::remove_dir_all(&tmp_include_dir);
                        }

                        match build_result {
                            Ok(build) => {
                                if build.status.success() {
                                    println!("{} Cartridge forged successfully!", "[+]".green());
                                    println!("Path :: {}", display_path.bright_black());
                                } else {
                                    println!(
                                        "{} Compilation failed:\n{}",
                                        "[-]".red(),
                                        String::from_utf8_lossy(&build.stderr)
                                    );
                                    exit(1);
                                }
                            }
                            Err(_) => {
                                println!(
                                    "{} FATAL: zig '{}' compiler not found in PATH.",
                                    "[-]".red().bold(),
                                    compiler
                                );
                                println!(
                                    "    {} ORE uses Zig as an ultra-fast LLVM C/C++ cross-compiler.",
                                    "[!]".yellow()
                                );
                                println!(
                                    "    {} Download it here: https://ziglang.org/download/\n",
                                    "[i]".bright_blue()
                                );
                                println!(
                                    "       Just run: zig {} -target wasm32-wasi {} -o {}",
                                    if is_cpp { "c++" } else { "cc" },
                                    filepath,
                                    dest_file.file_name().unwrap().to_str().unwrap()
                                );
                                exit(1);
                            }
                        }
                    }
                    _ => {
                        println!(
                            "{} FATAL: Unsupported file extension '{}'. Supported: .rs, .go, .py, .js, .ts, .zig, .c, .cpp, .cc, .cxx",
                            "[-]".red(),
                            ext
                        );
                        exit(1);
                    }
                }
            }
        }
    }
}
