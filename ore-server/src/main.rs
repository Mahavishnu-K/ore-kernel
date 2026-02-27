use axum::{
    extract::{Path, State, Json},
    routing::get,
    routing::post,
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use serde::{Deserialize, Serialize};
use ore_core::firewall::ContextFirewall;
use ore_core::driver::{InferenceDriver, OllamaDriver};
use ore_core::registry::AppRegistry;
use tokio::sync::Semaphore;

// kernel state shared across handlers
struct KernelState {
    ollama_url: String,
    model_name: String,
    gpu_lock: Arc<Semaphore>,
    registry: AppRegistry,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ORE SYSTEM KERNEL BOOTING ===");
    println!("-> Sweeping /manifests for installed Apps...");
    let app_registry = AppRegistry::boot_load("../manifests")
        .expect("FATAL: Failed to initialize App Registry");

    // configuration
    let shared_state = Arc::new(KernelState {
        ollama_url: "http://127.0.0.1:11434".to_string(),
        model_name: "llama3.2:1b".to_string(),
        gpu_lock: Arc::new(Semaphore::new(1)), // 1 concurrent GPU job
        registry: app_registry,
    });

    
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ask/:prompt", get(ask_ai))
        .route("/ps", get(process_status))
        .route("/ls", get(list_models))
        .route("/agents", get(list_agents))       
        .route("/manifests", get(list_manifests))
        .route("/expel/:model", get(expel_model))
        .route("/pull/:model", get(pull_model))
        .route("/load/:model", get(load_model)) 
        .route("/run", post(run_process))
        .with_state(shared_state);

    let addr = "127.0.0.1:3000";
    println!("=== ORE KERNEL IS ONLINE ===");
    println!("Listening on http://{}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "ORE Kernel is ALIVE. Connected to Ollama Backend."
}

// ollama requests/responses structs
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool, 
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
    done: bool,
}

#[derive(serde::Deserialize)]
pub struct RunRequest {
    pub model: String,
    pub prompt: String,
}

// inference engine (The Proxy & Firewall)
async fn ask_ai(
    State(state): State<Arc<KernelState>>,
    Path(prompt): Path<String>,
) -> String {
    let clean_prompt = prompt.replace("_", " ");
    
    println!("\n========================================");
    println!("-> Incoming App Request: {}", clean_prompt);

    let app_id = "openclaw"; // In the future, this comes from an API Key/Token
    let manifest = match state.registry.get_app(app_id) {
        Some(m) => m,
        None => return format!("ORE KERNEL ALERT: Unregistered Agent '{}'.", app_id),
    };
    
    let secured_prompt = match ContextFirewall::secure_request(manifest, &clean_prompt) {
        Ok(safe_text) => {
            println!("-> Security Check Passed.");
            if safe_text != clean_prompt {
                println!("-> [NOTICE] PII Redacted from prompt.");
            }
            safe_text
        },
        Err(e) => {
            println!("-> [BLOCKED] {}", e);
            return format!("ORE KERNEL ALERT: {}", e);
        }
    };

    println!("-> Waiting for GPU availability...");

    // the GPU scheduler
    let _permit = state.gpu_lock.acquire().await.unwrap();
    println!("-> GPU Lock Acquired! Routing to Ollama Driver...");

    let client = reqwest::Client::new();
    let request_body = OllamaRequest {
        model: state.model_name.clone(),
        prompt: secured_prompt, 
        stream: false,
    };

    let res = client
        .post(format!("{}/api/generate", state.ollama_url))
        .json(&request_body)
        .send()
        .await;

    match res {
        Ok(response) => {
            match response.json::<OllamaResponse>().await {
                Ok(json) => {
                    println!("-> Response received from Driver.");
                    println!("========================================");
                    return json.response;
                }
                Err(_) => return "Kernel Error: Failed to parse AI response.".to_string(),
            }
        }
        Err(_) => return "Kernel Error: Ollama Driver is offline. Is Ollama running?".to_string(),
    }
}

async fn run_process(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<RunRequest>,
) -> String {
    println!("\n========================================");
    println!("-> [EXEC] Model: {} | Prompt: {}", payload.model, payload.prompt);

    let app_id = "terminal_user"; 
    let manifest = match state.registry.get_app(app_id) {
        Some(m) => m,
        None => return format!("ORE KERNEL ALERT: Unregistered User '{}'.", app_id),
    };
    
    let secured_prompt = match ContextFirewall::secure_request(manifest, &payload.prompt) {
        Ok(safe_text) => {
            println!("-> Security Check Passed.");
            safe_text
        },
        Err(e) => {
            println!("-> [BLOCKED] {}", e);
            return format!("ORE KERNEL ALERT: {}", e);
        }
    };

    println!("-> Waiting for GPU availability...");

    let _permit = state.gpu_lock.acquire().await.unwrap();
    println!("-> GPU Lock Acquired! Executing on {}...", payload.model);

    let client = reqwest::Client::new();
    let request_body = OllamaRequest {
        model: payload.model.clone(), 
        prompt: secured_prompt,       
        stream: false,
    };

    // 4. Send to Driver
    let res = client
        .post(format!("{}/api/generate", state.ollama_url))
        .json(&request_body)
        .send()
        .await;

    match res {
        Ok(response) => {
            match response.json::<OllamaResponse>().await {
                Ok(json) => {
                    println!("-> Execution complete. Releasing GPU Lock.");
                    println!("========================================");
                    json.response
                }
                Err(_) => "Kernel Error: Failed to parse AI response.".to_string(),
            }
        }
        Err(_) => "Kernel Error: Driver is offline.".to_string(),
    }
}

async fn process_status() -> String {
    let driver = OllamaDriver::new("http://127.0.0.1:11434");
    
    match driver.get_running_models().await {
        Ok(models) => {
            let mut output = format!("{:<25} | {:<12} | {:<12}\n", "MODEL", "TOTAL RAM", "GPU VRAM");
            output.push_str("----------------------------------------------------------\n");
            
            if models.is_empty() {
                output.push_str("No models currently loaded in memory.\n");
            } else {
                for m in models {
                    // Convert bytes to Megabytes
                    let total_mb = m.size_bytes / 1024 / 1024;
                    let vram_mb = m.size_vram_bytes / 1024 / 1024;
                    
                    output.push_str(&format!(
                        "{:<25} | {:<9} MB | {:<9} MB\n", 
                        m.model_name, total_mb, vram_mb
                    ));
                }
            }
            output
        }
        Err(e) => format!("Kernel Error: {}", e),
    }
}

async fn list_models() -> String {
    let driver = OllamaDriver::new("http://127.0.0.1:11434");
    
    match driver.list_local_models().await {
        Ok(models) => {
            // Linux 'docker images' style formatting
            let mut output = format!("{:<25} | {:<10} | {}\n", "REPOSITORY", "SIZE", "UPDATED");
            output.push_str("------------------------------------------------------\n");
            
            if models.is_empty() {
                output.push_str("No models installed. Use 'ore install <model>'.\n");
            } else {
                for m in models {
                    let gb = m.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
                    output.push_str(&format!(
                        "{:<25} | {:.2} GB   | {}\n", 
                        m.name, gb, m.modified_at
                    ));
                }
            }
            output
        }
        Err(e) => format!("Kernel Error: {}", e),
    }
}

async fn expel_model(Path(model_name): Path<String>) -> String {
    println!("-> [KERNEL COMMAND] Received SIGKILL for model '{}'.", model_name);
    
    let driver = OllamaDriver::new("http://127.0.0.1:11434");
    
    match driver.unload_model(&model_name).await {
        Ok(_) => {
            println!("-> [SUCCESS] VRAM flushed. Model '{}' evicted.", model_name);
            format!("SUCCESS: Model '{}' has been forcefully evicted from GPU VRAM.", model_name)
        }
        Err(e) => {
            println!("-> [ERROR] Failed to flush VRAM: {}", e);
            format!("KERNEL ERROR: Could not evict model. {}", e)
        }
    }
}

async fn pull_model(Path(model_name): Path<String>) -> String {
    println!("-> [PACKAGE MANAGER] Instructing driver to install model '{}'...", model_name);
    
    let driver = OllamaDriver::new("http://127.0.0.1:11434");
    
    match driver.pull_model(&model_name).await {
        Ok(_) => {
            println!("-> [SUCCESS] Model '{}' successfully installed to local hardware.", model_name);
            format!("SUCCESS: Model '{}' installed and ready for inference.", model_name)
        }
        Err(e) => {
            println!("-> [ERROR] Installation failed: {}", e);
            format!("KERNEL ERROR: Could not install model. {}", e)
        }
    }
}

async fn load_model(Path(model_name): Path<String>) -> String {
    println!("-> [KERNEL COMMAND] Allocating VRAM and pre-loading '{}'...", model_name);
    
    let driver = OllamaDriver::new("http://127.0.0.1:11434");
    
    match driver.preload_model(&model_name).await {
        Ok(_) => {
            println!("-> [SUCCESS] Model '{}' locked into VRAM.", model_name);
            format!("SUCCESS: Model '{}' is now pre-loaded and ready for zero-latency inference.", model_name)
        }
        Err(e) => {
            println!("-> [ERROR] Failed to allocate VRAM: {}", e);
            format!("KERNEL ERROR: Could not load model. {}", e)
        }
    }
}

async fn list_agents(State(state): State<Arc<KernelState>>) -> String {
    let apps = state.registry.list_apps();
    
    let mut output = format!("{:<20} | {:<10} | {:<20} | {:<10} | {}\n", 
        "AGENT ID", "VERSION", "ALLOWED MODELS", "PRIORITY", "STATUS");
    output.push_str("----------------------------------------------------------------------------------\n");
    
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
            let models_disp = if models.len() > 17 { format!("{}...", &models[..17]) } else { models };

            // 2. Handle Empty Priority
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
                app.app_id, 
                app.version,
                models_disp,
                priority,
                status
            ));
        }
    }
    output
}

async fn list_manifests(State(state): State<Arc<KernelState>>) -> String {
    let apps = state.registry.list_apps();
    
    let mut output = format!("{:<20} | {:<10} | {:<12} | {:<15} | {}\n", 
        "MANIFEST FILE", "NETWORK", "FILE I/O", "EXECUTION", "PII SCRUBBING");
    output.push_str("------------------------------------------------------------------------------------\n");
    
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
                if app.network.network_enabled { "ENABLED" } else { "BLOCKED" },
                fs_status,
                exec_status,
                pii_status
            ));
        }
    }
    output
}