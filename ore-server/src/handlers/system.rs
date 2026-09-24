use crate::payloads::ExecuteRequest;
use crate::state::KernelState;
use axum::extract::{Json, Path, State};
use ore_core::crypto::KernelCrypto;
use ore_core::kprintln;
use ore_core::memory::Pager;
use ore_core::sandbox::ExecuteParams;
use std::fs;
use std::sync::Arc;

pub async fn health_check(State(state): State<Arc<KernelState>>) -> String {
    format!(
        "ORE Kernel is ALIVE. Powered by: {}",
        state.driver.engine_name()
    )
    .to_string()
}

pub async fn execute_tool(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<ExecuteRequest>,
) -> String {
    kprintln!(
        "-> [EXECUTION] Agent '{}' requested to run a sandbox.",
        payload.app_id,
    );

    let manifest = match state.registry.get_app(&payload.app_id) {
        Some(m) => m,
        None => {
            return format!(
                "KERNEL ALERT: Unregistered Agent '{}'. Access Denied.",
                payload.app_id
            );
        }
    };

    let has_wasm_tool =
        payload.tool_name.is_some() || payload.args.is_some() || payload.input_data.is_some();
    let has_wasm_script = payload.language.is_some() || payload.script.is_some();
    let has_shell = payload.shell_command.is_some();

    if (has_wasm_tool as u8 + has_wasm_script as u8 + has_shell as u8) > 1 {
        kprintln!(
            "-> [BLOCKED] Ambiguous execution payload from Agent '{}'.",
            manifest.app_id
        );
        return "KERNEL ERROR: Ambiguous request. Choose one mode (tool, script, or shell)."
            .to_string();
    }

    // Raw host shell execution mode
    if let Some(cmd) = &payload.shell_command {
        // STRICT MANIFEST ENFORCEMENT
        if !manifest.execution.can_execute_shell {
            kprintln!(
                "-> [BLOCKED] Agent '{}' lacks raw SHELL execution permissions.",
                manifest.app_id
            );
            return "KERNEL ALERT: Permission Denied. can_execute_shell is false.".to_string();
        }

        kprintln!(
            "-> [WARN] Agent '{}' executing RAW HOST SHELL command...",
            manifest.app_id
        );

        // SPAWN THE HOST PROCESS (Bypasses Sandbox entirely)
        // Automatically uses 'cmd.exe' for Windows, and 'sh' for Linux/macOS
        let output = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd").args(["/C", cmd]).output()
        } else {
            std::process::Command::new("sh").arg("-c").arg(cmd).output()
        };

        // CAPTURE AND RETURN HOST OUTPUT
        return match output {
            Ok(out) => {
                let mut final_output = String::from_utf8_lossy(&out.stdout).to_string();
                let error_output = String::from_utf8_lossy(&out.stderr).to_string();

                if !error_output.is_empty() {
                    final_output.push_str("\n--- STDERR ---\n");
                    final_output.push_str(&error_output);
                }

                kprintln!("-> [SHELL SUCCESS] Output returned to Agent.");
                final_output
            }
            Err(e) => {
                kprintln!("-> [SHELL FAILED] {}", e);
                format!("KERNEL ERROR: Host Shell execution failed: {}", e).to_string()
            }
        };
    }

    if !manifest.execution.can_execute_wasm {
        kprintln!(
            "-> [BLOCKED] Agent '{}' lacks WASM execution permissions.",
            manifest.app_id
        );
        return "KERNEL ALERT: Permission Denied. can_execute_wasm is false in manifest."
            .to_string();
    }

    let base_dir = match std::path::absolute(ore_core::get_ore_dir()) {
        Ok(p) => p,
        Err(e) => return format!("KERNEL ERROR: Cannot resolve ORE base dir: {}", e),
    };
    let wasm_path: std::path::PathBuf;
    let mut run_args = vec![];
    let mut inception_data = None;
    let mut dynamic_vfs_mounts = Vec::new();

    // Define a variable to hold the hash so we only calculate it ONCE.
    let mut resolved_req_hash: Option<String> = None;

    if let Some(script) = &payload.script {
        let lang = payload.language.as_deref().unwrap_or("python");
        kprintln!("-> [EXECUTION] Mode: Autonomous Script ({})", lang);

        if !manifest
            .execution
            .allowed_language_runtimes
            .contains(&lang.to_string())
            && !manifest
                .execution
                .allowed_language_runtimes
                .contains(&"*".to_string())
        {
            kprintln!(
                "-> [BLOCKED] Runtime '{}' is not in allowed_language_runtimes list.",
                lang
            );
            return format!(
                "KERNEL ALERT: Autonomous scripting in '{}' is not whitelisted. Add it to allowed_language_runtimes.",
                lang
            );
        }

        if lang == "python" || lang == "py" {
            wasm_path = base_dir.join("runtimes").join("system-py.wasm");

            // JIT PIP VENDORING FOR AUTONOMOUS SCRIPTS
            if let Some(deps) = &payload.dependencies
                && !deps.is_empty()
            {
                crate::kprintln!("-> [EXECUTION] AI requested dependencies: {:?}", deps);

                // Create a deterministic hash for this exact combination of packages
                let mut sorted_deps = deps.clone();
                sorted_deps.sort();
                let req_string = sorted_deps.join(",");

                // Mathematically perfect SHA-256
                let hash_bytes = KernelCrypto::sha256(req_string.as_bytes());
                // Native Rust Hex Conversion
                let req_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                resolved_req_hash = Some(req_hash.clone());

                let cache_dir = base_dir.join("cache").join("pip").join(&req_hash);

                // If it's not cached, tell the Host OS to download them!
                if !cache_dir.exists() {
                    crate::kprintln!("-> [KERNEL] Cache miss. Host OS downloading packages...");

                    if let Err(e) = fs::create_dir_all(&cache_dir) {
                        crate::kprintln!(
                            "-> [KERNEL ERROR] Failed to create pip cache directory '{}': {}",
                            cache_dir.display(),
                            e
                        );

                        return format!(
                            "KERNEL ERROR: Failed to create pip cache directory '{}': {}",
                            cache_dir.display(),
                            e
                        );
                    }

                    let python_cmd = if cfg!(target_os = "windows") {
                        "python"
                    } else {
                        "python3"
                    };

                    let cache_path = cache_dir.to_string_lossy().to_string();

                    crate::kprintln!(
                        "-> [JIT PIP] Launching '{}' for dependencies: {:?}",
                        python_cmd,
                        deps
                    );

                    crate::kprintln!("-> [JIT PIP] Target directory: {}", cache_path);

                    crate::kprintln!(
                        "-> [JIT PIP] PATH: {}",
                        std::env::var("PATH").unwrap_or_else(|_| "<PATH unavailable>".to_string())
                    );

                    let mut pip_install = std::process::Command::new(python_cmd);

                    pip_install
                        .args(["-m", "pip", "install", "--target", &cache_path])
                        .args(deps); // Pass the AI's requested packages

                    let pip_cmd = match pip_install.output() {
                        Ok(output) => output,
                        Err(e) => {
                            let _ = fs::remove_dir_all(&cache_dir); // Cleanup
                            crate::kprintln!("-> [KERNEL ERROR] Failed to launch pip: {}", e);
                            return format!(
                                "KERNEL ERROR: Failed to launch pip: {}. \
                                Make sure Python and pip are installed and available in the ORE server PATH.",
                                e
                            );
                        }
                    };

                    if !pip_cmd.status.success() {
                        crate::kprintln!("-> [KERNEL ERROR] Failed to install AI dependencies.");
                        let _ = fs::remove_dir_all(&cache_dir); // Cleanup
                        return format!(
                            "KERNEL ERROR: Failed to resolve requirements: {}",
                            String::from_utf8_lossy(&pip_cmd.stderr)
                        );
                    }

                    // C-Extension Scanner (Fail-Fast Security)
                    for entry in walkdir::WalkDir::new(&cache_dir) {
                        let entry = entry.unwrap();
                        if entry.path().is_file() {
                            let ext = entry
                                .path()
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("");
                            if ["so", "pyd", "dylib", "dll"].contains(&ext) {
                                let _ = fs::remove_dir_all(&cache_dir);
                                return format!(
                                    "KERNEL ALERT: The AI requested a package containing illegal C-Extensions ({}). Denied.",
                                    entry.path().display()
                                );
                            }
                        }
                    }
                } else {
                    crate::kprintln!("-> [KERNEL] Packages found in ORE Cache.");
                }

                // Dynamically mount the Cache into the Sandbox!
                // We add the cache directory to `allowed_read_paths` so sandbox.rs automatically mounts it!
                dynamic_vfs_mounts.push(cache_dir.to_string_lossy().to_string());
            }

            run_args.push("python".to_string());
            run_args.push("/ore_tmp/inception.py".to_string()); // Point interpreter to VFS
            inception_data = Some(("inception.py".to_string(), script.clone()));
        } else if lang == "javascript" || lang == "js" || lang == "ts" || lang == "typescript" {
            wasm_path = base_dir.join("runtimes").join("system-js.wasm");
            run_args.push("quickjs".to_string());

            let ext = if lang.starts_with("ts") || lang == "typescript" {
                "ts"
            } else {
                "js"
            };
            let filename = format!("inception.{}", ext);

            // THE ORE KERNEL ROUTER (Force custom VFS Polyfills)
            // A comprehensive, O(1) lookup table of every polyfill in your js_modules folder
            let core_modules: std::collections::HashSet<&str> = [
                "assert",
                "buffer",
                "crypto",
                "encoding",
                "events",
                "fs",
                "fs/promises",
                "http",
                "https",
                "os",
                "path",
                "process",
                "punycode",
                "querystring",
                "stream",
                "stream/consumers",
                "stream/promises",
                "string_decoder",
                "timers",
                "timers/promises",
                "url",
                "util",
                "util/types",
                "whatwg_url",
            ]
            .iter()
            .cloned()
            .collect();

            // A SINGLE Regex that catches all variations in one pass:
            // 1. import { x } from 'fs'
            // 2. import fs from "node:fs"
            // 3. await import('fs')
            // 4. require('fs')
            let import_re = regex::Regex::new(
                r#"(?m)(import\s+(?:[a-zA-Z0-9_\{\}\*,\s]+\s+from\s+)?|import\s*\(\s*|require\s*\(\s*)['"](?:node:)?([a-zA-Z0-9_/-]+)['"](\s*\)?)"#
            ).unwrap();

            let routed_script = import_re
                .replace_all(script, |caps: &regex::Captures| {
                    let prefix = &caps[1]; // e.g., "import fs from "
                    let mut mod_name = &caps[2]; // e.g., "fs", "fs/promises", "axios"
                    let suffix = &caps[3]; // e.g., ")" for requires, or "" for imports

                    // Automatically route 'https' to your 'http.js' polyfill
                    if mod_name == "https" {
                        mod_name = "http";
                    }

                    // Only rewrite the path if it is one of our ORE polyfills!
                    if core_modules.contains(mod_name) {
                        format!("{}'/modules/{}.js'{}", prefix, mod_name, suffix)
                    } else {
                        caps[0].to_string() // Not a core module (e.g. 'lodash'), leave it untouched for NPM!
                    }
                })
                .to_string();

            let mut final_script = routed_script.clone();

            // JAVASCRIPT: JIT NPM CACHING & ESBUILD INJECTION
            if let Some(deps) = &payload.dependencies
                && !deps.is_empty()
            {
                crate::kprintln!("-> [JIT NPM] AI requested dependencies: {:?}", deps);

                let mut sorted = deps.clone();
                sorted.sort();
                let req_string = sorted.join(",");

                // Mathematically perfect SHA-256
                let hash_bytes = KernelCrypto::sha256(req_string.as_bytes());
                // Native Rust Hex Conversion
                let req_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                let cache_dir = base_dir.join("cache").join("npm").join(&req_hash);

                // Download to Cache (Only if missing!)
                if !cache_dir.exists() {
                    crate::kprintln!("-> [JIT NPM] Cache miss. Host OS downloading packages...");
                    fs::create_dir_all(&cache_dir).unwrap();
                    fs::write(
                        cache_dir.join("package.json"),
                        r#"{"name":"ore-jit","version":"1.0.0"}"#,
                    )
                    .unwrap();

                    // THE BULLETPROOF WINDOWS SUBPROCESS LAUNCHER
                    let mut npm_install = if cfg!(target_os = "windows") {
                        let mut cmd = std::process::Command::new("cmd");
                        cmd.arg("/C").arg("npm").arg("install");
                        cmd
                    } else {
                        let mut cmd = std::process::Command::new("npm");
                        cmd.arg("install");
                        cmd
                    };

                    npm_install.current_dir(&cache_dir);
                    for dep in deps {
                        npm_install.arg(dep);
                    }

                    let npm_output = match npm_install.output() {
                        Ok(output) => output,
                        Err(e) => {
                            let _ = fs::remove_dir_all(&cache_dir);

                            crate::kprintln!("-> [JIT NPM ERROR] Failed to launch npm: {}", e);

                            return format!(
                                "KERNEL ERROR: Failed to launch npm: {}. \
                                Make sure Node.js/npm is installed and available in the ORE server PATH.",
                                e
                            );
                        }
                    };

                    if !npm_output.status.success() {
                        let _ = fs::remove_dir_all(&cache_dir);

                        crate::kprintln!(
                            "-> [JIT NPM ERROR] npm install failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
                            String::from_utf8_lossy(&npm_output.stdout),
                            String::from_utf8_lossy(&npm_output.stderr),
                        );

                        return format!(
                            "KERNEL ERROR: Failed to install NPM dependencies:\n{}",
                            String::from_utf8_lossy(&npm_output.stderr)
                        );
                    }
                } else {
                    crate::kprintln!("-> [JIT NPM] Cache hit! Bypassing npm install.");
                }

                // Fast Bundling (Always happens, takes < 5ms)
                // Prevent Race Conditions with Unique IDs!
                let run_id = uuid::Uuid::new_v4().to_string();
                let entry_file = cache_dir.join(format!("index_{}.{}", run_id, ext));
                let out_file = cache_dir.join(format!("bundle_{}.js", run_id));

                fs::write(&entry_file, &routed_script).unwrap();

                // THE BULLETPROOF WINDOWS SUBPROCESS LAUNCHER
                let mut esbuild = if cfg!(target_os = "windows") {
                    let mut cmd = std::process::Command::new("cmd");
                    cmd.arg("/C").arg("npx").arg("esbuild");
                    cmd
                } else {
                    let mut cmd = std::process::Command::new("npx");
                    cmd.arg("esbuild");
                    cmd
                };

                esbuild
                    .current_dir(&cache_dir)
                    .args([
                        &format!("index_{}.{}", run_id, ext), // Use the unique entry file!
                        "--bundle",
                        "--format=esm",
                    ])
                    .arg(format!("--outfile={}", out_file.to_string_lossy()));

                // THE NPM ALIAS ENGINE
                // If a downloaded NPM package like 'axios' or 'pdf-lib' secretly calls require('fs')
                // deep inside its own code, esbuild will intercept it and route it to our VFS polyfill!
                for core in core_modules.iter() {
                    let polyfill_path = if *core == "https" {
                        "/modules/http.js".to_string() // Route 'https' to our 'http.js' polyfill
                    } else {
                        format!("/modules/{}.js", core)
                    };
                    esbuild.arg(format!("--alias:{}={}", core, polyfill_path));
                    esbuild.arg(format!("--alias:node:{}={}", core, polyfill_path));
                }

                // Tell esbuild that anything starting with /modules/ is external (already inside the WASM VFS)
                esbuild.arg("--external:/modules/*");

                let build_res = esbuild.output().unwrap();
                if build_res.status.success() {
                    final_script = std::fs::read_to_string(&out_file).unwrap();
                    crate::kprintln!("-> [JIT NPM] Script successfully bundled.");

                    let _ = std::fs::remove_file(entry_file);
                    let _ = std::fs::remove_file(out_file);
                } else {
                    let _ = std::fs::remove_file(entry_file);
                    let _ = std::fs::remove_file(out_file);
                    return format!(
                        "KERNEL ERROR: JIT NPM Bundler failed: {}",
                        String::from_utf8_lossy(&build_res.stderr)
                    );
                }
            }

            run_args.push(format!("/ore_tmp/{}", filename)); // Point QuickJS to VFS
            inception_data = Some((filename, final_script));
        } else {
            return format!("KERNEL ERROR: Unsupported language '{}'", lang);
        }
    } else if let Some(tool) = &payload.tool_name {
        kprintln!("-> [EXECUTION] Mode: Fixed Tool ({}.wasm)", tool);

        if !manifest.execution.allowed_tools.contains(tool)
            && !manifest.execution.allowed_tools.contains(&"*".to_string())
        {
            kprintln!("-> [BLOCKED] Tool '{}' is not in allowed_tools list.", tool);
            return format!(
                "KERNEL ALERT: Tool '{}' is not whitelisted in manifest. Add it to allowed_tools.",
                tool
            );
        }

        // LOAD THE CARTRIDGE ("The Console-Cartridge Architecture")
        // We look for the pre-compiled .wasm file in a local /tools directory
        wasm_path = base_dir.join("tools").join(format!("{}.wasm", tool));
        run_args.push(tool.clone()); // argv[0]

        let args_path = base_dir.join("tools").join(format!("{}.args", tool));
        if args_path.exists()
            && let Ok(default_args_str) = std::fs::read_to_string(&args_path)
        {
            // We read line-by-line so arguments with spaces don't get broken
            for line in default_args_str.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    run_args.push(trimmed.to_string());
                }
            }
        }

        if let Some(args) = &payload.args {
            run_args.extend(args.clone());
        }
    } else {
        return "KERNEL ERROR: Must provide either 'script' or 'tool_name'.".to_string();
    }

    if !wasm_path.exists() {
        return format!(
            "KERNEL ERROR: Tool binary '{}' not found. Run 'ore pull <tool>' or install the tool.",
            wasm_path.display()
        );
    }

    let wasm_binary = match fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => return format!("KERNEL ERROR: Failed to read WASM binary: {}", e),
    };

    let cache_key = if let Ok(metadata) = fs::metadata(&wasm_path) {
        if let Ok(modified) = metadata.modified() {
            let duration = modified
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            // e.g., "python_vfs_test_1710429100"
            format!(
                "{}_{}",
                wasm_path.file_stem().unwrap().to_str().unwrap(),
                duration.as_secs()
            )
        } else {
            // Fallback if OS doesn't support modified times
            format!(
                "{}_static",
                wasm_path.file_stem().unwrap().to_str().unwrap()
            )
        }
    } else {
        "unknown_static".to_string()
    };

    let resolve_path = |p: &String| -> String {
        let path = std::path::Path::new(p);
        if path.is_absolute() {
            p.clone()
        } else {
            // Anchor relative paths to the ORE Base Dir!
            base_dir.join(path).to_string_lossy().to_string()
        }
    };

    let mut resolved_read_paths: Vec<String> = manifest
        .file_system
        .allowed_read_paths
        .iter()
        .map(resolve_path)
        .collect();

    resolved_read_paths.extend(dynamic_vfs_mounts);

    let resolved_write_paths: Vec<String> = manifest
        .file_system
        .allowed_write_paths
        .iter()
        .map(resolve_path)
        .collect();

    let params = ExecuteParams {
        tool_name: wasm_path.file_stem().unwrap().to_str().unwrap().to_string(),
        wasm_binary,
        cache_key,
        fuel_limit: manifest.execution.max_cpu_instructions, // Dynamic fuel limit per manifest (Default: 5 Billion ≈ 2 seconds of pure compute)
        args: run_args,
        stdin: payload.input_data.map(|s| s.into_bytes()),
        inception: inception_data,
        allowed_read_paths: resolved_read_paths,
        allowed_write_paths: resolved_write_paths,
        network_enabled: manifest.network.network_enabled,
        allow_localhost_access: manifest.network.allow_localhost_access,
        network_rules: manifest.network.rules.clone(),
        dynamic_vfs_path: resolved_req_hash.map(|hash| format!("/workspace/{}", hash)),
        wasm_path: wasm_path.clone(),
    };

    let sandbox = state.sandbox.clone();

    let exec_result = tokio::task::spawn_blocking(move || sandbox.execute(params)).await;

    match exec_result {
        Ok(Ok(output)) => {
            ore_core::kprintln!("-> [EXECUTION SUCCESS] Output returned to Agent.");
            output
        }
        Ok(Err(e)) => {
            ore_core::kprintln!("-> [EXECUTION FAILED] {}", e);
            format!("KERNEL ERROR: {}", e).to_string()
        }
        Err(e) => {
            ore_core::kprintln!("-> [KERNEL PANIC] Sandbox thread crashed: {}", e);
            format!("KERNEL PANIC: {}", e).to_string()
        }
    }
}

pub async fn process_status(State(state): State<Arc<KernelState>>) -> String {
    match state.driver.get_running_models().await {
        Ok(models) => {
            let mut output = format!(
                "{:<25} | {:<12} | {:<12}\n",
                "MODEL", "TOTAL RAM", "GPU VRAM"
            );
            output.push_str("----------------------------------------------------------\n");

            if models.is_empty() {
                output.push_str("No models currently loaded in memory.\n");
            } else {
                for m in models {
                    output.push_str(&format!(
                        "{:<25} | {:<9} MB | {:<9} MB\n",
                        m.model_name,
                        m.size_bytes / 1024 / 1024,
                        m.size_vram_bytes / 1024 / 1024
                    ));
                }
            }
            output
        }
        Err(e) => format!("Kernel Error: {}", e),
    }
}

pub async fn list_models(State(state): State<Arc<KernelState>>) -> String {
    match state.driver.list_local_models().await {
        Ok(models) => {
            let mut output = format!("{:<25} | {:<10} | {}\n", "REPOSITORY", "SIZE", "UPDATED");
            output.push_str("------------------------------------------------------\n");
            if models.is_empty() {
                output.push_str("No models installed. Use 'ore pull <model>'.\n");
            } else {
                for m in models {
                    output.push_str(&format!(
                        "{:<25} | {:.2} GB   | {}\n",
                        m.name,
                        m.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
                        m.modified_at
                    ));
                }
            }
            output
        }
        Err(e) => format!("Kernel Error: {}", e),
    }
}

pub async fn expel_model(
    State(state): State<Arc<KernelState>>,
    Path(model_name): Path<String>,
) -> String {
    match state.driver.unload_model(&model_name).await {
        Ok(_) => format!(
            "SUCCESS: Model '{}' has been forcefully evicted from GPU VRAM.",
            model_name
        ),
        Err(e) => format!("KERNEL ERROR: {}", e),
    }
}

pub async fn pull_model(
    State(state): State<Arc<KernelState>>,
    Path(model_name): Path<String>,
) -> String {
    match state.driver.pull_model(&model_name).await {
        Ok(_) => format!("SUCCESS: Model '{}' installed.", model_name),
        Err(e) => format!("KERNEL ERROR: {}", e),
    }
}

pub async fn load_model(
    State(state): State<Arc<KernelState>>,
    Path(model_name): Path<String>,
) -> String {
    match state.driver.preload_model(&model_name).await {
        Ok(_) => format!("SUCCESS: Model '{}' loaded.", model_name),
        Err(e) => format!("KERNEL ERROR: {}", e),
    }
}

pub async fn list_agents(State(state): State<Arc<KernelState>>) -> String {
    let apps = state.registry.list_apps();

    let mut output = format!(
        "{:<20} | {:<10} | {:<20} | {:<10} | {}\n",
        "AGENT ID", "VERSION", "ALLOWED MODELS", "PRIORITY", "STATUS"
    );
    output.push_str(
        "----------------------------------------------------------------------------------\n",
    );

    if apps.is_empty() {
        output.push_str("No agents registered. Use 'ore manifest <name>' to create one.\n");
    } else {
        for app in apps {
            // 1. Handle Empty Models
            let models = if app.resources.allowed_models.is_empty() {
                "-".to_string()
            } else {
                app.resources.allowed_models.join(", ")
            };

            // Truncate if too long
            let models_disp = if models.len() > 17 {
                format!("{}...", &models[..17]).to_string()
            } else {
                models
            };

            // Handle Empty Priority
            // If the string is empty, show "-", otherwise UPPERCASE it.
            let priority = if app.resources.gpu_priority.trim().is_empty() {
                "-".to_string()
            } else {
                app.resources.gpu_priority.to_uppercase()
            };

            let status = if app.execution.can_execute_shell || !app.privacy.enforce_pii_redaction {
                "UNSAFE"
            } else if app.resources.allowed_models.is_empty() && !app.network.network_enabled {
                "DORMANT"
            } else {
                "SECURED"
            };

            output.push_str(&format!(
                "{:<20} | {:<10} | {:<20} | {:<10} | {}\n",
                app.app_id, app.version, models_disp, priority, status
            ));
        }
    }
    output
}

pub async fn list_manifests(State(state): State<Arc<KernelState>>) -> String {
    let apps = state.registry.list_apps();

    let mut output = format!(
        "{:<20} | {:<10} | {:<12} | {:<15} | {}\n",
        "MANIFEST FILE", "NETWORK", "FILE I/O", "EXECUTION", "PII SCRUBBING"
    );
    output.push_str(
        "------------------------------------------------------------------------------------\n",
    );

    if apps.is_empty() {
        output.push_str("No manifests found in /manifests directory.\n");
    } else {
        for app in apps {
            let can_read = !app.file_system.allowed_read_paths.is_empty();
            let can_write = !app.file_system.allowed_write_paths.is_empty();
            let fs_status = match (can_read, can_write) {
                (true, true) => "Read/Write",
                (true, false) => "Read-Only",
                (false, true) => "Write-Only",
                (false, false) => "Air-gapped",
            };

            let exec_status = if app.execution.can_execute_shell {
                "SHELL (RISK)"
            } else if app.execution.can_execute_wasm {
                "WASM Sandbox"
            } else {
                "Disabled"
            };

            let pii_status = if app.privacy.enforce_pii_redaction {
                "ACTIVE"
            } else {
                "OFF (RISK)"
            };

            output.push_str(&format!(
                "{:<20} | {:<10} | {:<12} | {:<15} | {}\n",
                format!("{}.toml", app.app_id),
                if app.network.network_enabled {
                    "ENABLED"
                } else {
                    "BLOCKED"
                },
                fs_status,
                exec_status,
                pii_status
            ));
        }
    }
    output
}

pub async fn compact_memory(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] Manual Memory Compaction triggered for Agent '{}'",
        app_id
    );

    let manifest = match state.registry.get_app(&app_id) {
        Some(m) => m.clone(),
        None => return format!("KERNEL ERROR: Unregistered Agent '{}'.", app_id),
    };

    if !manifest.resources.json_history {
        return format!(
            "KERNEL ERROR: Agent '{}' does not use JSON history. Cannot compact.",
            app_id
        );
    }

    let history = Pager::page_in_history(&app_id);
    if history.len() <= 2 {
        return "SUCCESS: History is already too short to compact.".to_string();
    }

    let target_model = manifest
        .resources
        .allowed_models
        .first()
        .map(|s| s.as_str())
        .unwrap_or("llama3.2:1b");
    let lease = state.scheduler.request_gpu(target_model, &app_id).await;

    let text_to_summarize = history
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<String>>()
        .join("\n");

    let summary_prompt = format!(
        "You are a system memory compressor. Condense the following conversation log into an ultra-short, dense summary. Keep ALL names, numbers, decisions, and strict facts. Discard all conversational filler. Output ONLY the raw facts in as few words as mathematically possible.\n\nRAW LOG:\n{}\n\nCOMPRESSED FACTS:",
        text_to_summarize
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let driver_clone = Arc::clone(&state.driver);
    let m_clone = target_model.to_string();
    let a_clone = app_id.clone();

    // Spawn the generation task
    tokio::spawn(async move {
        let _ = driver_clone
            .generate_text(&m_clone, &a_clone, false, &summary_prompt, None, tx, "")
            .await;
    });

    let mut summary = String::new();
    while let Some(word) = rx.recv().await {
        summary.push_str(&word);
    }

    drop(lease); // Release GPU

    let mut compacted_history = Vec::new();
    compacted_history.push(ore_core::memory::ContextMessage {
        role: "system".to_string(),
        content: format!(
            "You are a helpful AI assistant. Previous context summary:\n{}",
            summary.trim()
        ),
    });

    let len = history.len();
    compacted_history.push(history[len - 2].clone());
    compacted_history.push(history[len - 1].clone());

    // Overwrite the SSD files
    Pager::page_out_history(&app_id, &compacted_history);

    if manifest.resources.stateful_paging {
        Pager::delete_kv_cache(&app_id);
    }

    format!("SUCCESS: Memory for Agent '{}' manually compacted.", app_id).to_string()
}

pub async fn clear_memory(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] Wiping SSD Memory for Agent '{}'",
        app_id
    );
    Pager::clear_page(&app_id);
    let _ = state.driver.invalidate_agent_cache(&app_id).await;
    format!(
        "SUCCESS: Memory for Agent '{}' has been wiped clean from SSD and RAM.",
        app_id
    )
    .to_string()
}

pub async fn top_telemetry(State(state): State<Arc<KernelState>>) -> String {
    let scheduler_status = state.scheduler.get_status().await;
    let apps_count = state.registry.list_apps().len();

    let mut output = "=== ORE KERNEL TELEMETRY ===\n".to_string();
    output.push_str(&format!("{:<20} | Status\n", "Subsystem"));
    output.push_str(&format!("{:<20} | ------\n", "-------------------"));
    output.push_str(&format!(
        "{:<20} | ACTIVE\n",
        format!("Driver ({})", state.driver.engine_name())
    ));
    output.push_str(&format!(
        "{:<20} | {}\n",
        "Scheduler (VRAM)", scheduler_status
    ));
    output.push_str(&format!("{:<20} | ENFORCING\n", "Context Firewall"));
    output.push_str(&format!("{:<20} | {}\n", "Connected Apps", apps_count));

    output
}

pub async fn kill_app(State(state): State<Arc<KernelState>>, Path(app_id): Path<String>) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] SIGTERM received for Agent '{}'",
        app_id
    );
    let _ = state.driver.invalidate_agent_cache(&app_id).await;
    format!("SUCCESS: App '{}' context wiped from GPU Memory.", app_id).to_string()
}
