use axum::{
    extract::{Request, Json, Path, State},
    middleware::{self, Next},
    response::Response,
    http::{StatusCode, HeaderMap},
    routing::get,
    routing::post,
    Router,
};
use ore_core::driver::{InferenceDriver, OllamaDriver};
use ore_core::firewall::ContextFirewall;
use ore_core::registry::AppRegistry;
use ore_core::ipc::{SemanticBus, RateLimiter, MessageBus, AgentMessage};
use ore_core::scheduler::GpuScheduler;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::fs;
use uuid::Uuid;
use tokio::net::TcpListener;

// kernel state shared across handlers
struct KernelState {
    ollama_url: String,
    scheduler: Arc<GpuScheduler>,
    registry: AppRegistry,
    semantic_bus: SemanticBus,
    message_bus: MessageBus,
    rate_limiter: RateLimiter,
    auth_token: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ORE SYSTEM KERNEL BOOTING ===");

    let session_token = Uuid::new_v4().to_string();
    fs::write("ore-kernel.token", &session_token).expect("Failed to write security token.");
    println!("-> [SECURITY] Master Token generated and secured to disk.");

    println!("-> Sweeping /manifests for installed Apps...");
    let app_registry =
        AppRegistry::boot_load("../manifests").expect("FATAL: Failed to initialize App Registry");

    // configuration
    let shared_state = Arc::new(KernelState {
        ollama_url: "http://127.0.0.1:11434".to_string(),
        scheduler: Arc::new(GpuScheduler::new()),
        registry: app_registry,
        semantic_bus: SemanticBus::new(),
        message_bus: MessageBus::new(),
        rate_limiter: RateLimiter::new(),
        auth_token: session_token,
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
        .route("/ipc/share", post(sys_share_context))
        .route("/ipc/search", post(sys_search_context))
        .route("/ipc/send", post(ipc_send))          
        .route("/ipc/listen/:app_id", get(ipc_listen))
        .layer(middleware::from_fn_with_state(shared_state.clone(), auth_middleware))
        .with_state(shared_state);

    let addr = "127.0.0.1:3000";
    println!("=== ORE KERNEL IS ONLINE ===");
    println!("Listening on http://{}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    let _ = fs::remove_file("ore-kernel.token");
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

#[derive(serde::Deserialize)]
pub struct IpcShareRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub knowledge_text: String, 
}

#[derive(serde::Deserialize)]
pub struct IpcSearchRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub query: String, 
}

// inference engine (The Proxy & Firewall)
async fn ask_ai(State(state): State<Arc<KernelState>>, Path(prompt): Path<String>) -> String {
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
        }
        Err(e) => {
            println!("-> [BLOCKED] {}", e);
            return format!("ORE KERNEL ALERT: {}", e);
        }
    };

    println!("-> Waiting for GPU Scheduler...");

    // If the agent lists allowed_models, pick the first one. Default to "llama3.2:1b"
    let target_model = manifest.resources.allowed_models.first()
        .map(|s| s.as_str())
        .unwrap_or("llama3.2:1b");

    // the GPU scheduler
    let lease = state.scheduler.request_gpu(target_model).await;
    println!("-> GPU Lease Granted for '{}'. Routing to Driver...", lease.model);

    let client = reqwest::Client::new();
    let request_body = OllamaRequest {
        model: lease.model.clone(),
        prompt: secured_prompt,
        stream: false,
    };

    let res = client
        .post(format!("{}/api/generate", state.ollama_url))
        .json(&request_body)
        .send()
        .await;

    match res {
        Ok(response) => match response.json::<OllamaResponse>().await {
            Ok(json) => {
                println!("-> Response received from Driver.");
                println!("========================================");
                return json.response;
            }
            Err(_) => return "Kernel Error: Failed to parse AI response.".to_string(),
        },
        Err(_) => return "Kernel Error: Ollama Driver is offline. Is Ollama running?".to_string(),
    }
}

async fn run_process(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<RunRequest>,
) -> String {
    println!("\n========================================");
    println!(
        "-> [EXEC] Model: {} | Prompt: {}",
        payload.model, payload.prompt
    );

    let app_id = "terminal_user";
    let manifest = match state.registry.get_app(app_id) {
        Some(m) => m,
        None => return format!("ORE KERNEL ALERT: Unregistered User '{}'.", app_id),
    };

    let limit = manifest.resources.max_tokens_per_minute;

    // Assuming ~500 tokens per request
    // future update: calculate tokens based on prompt length or use a more dynamic approach
    if !state.rate_limiter.check_and_add(app_id, limit, 500) {
        println!("-> [BLOCKED] Agent '{}' exceeded GPU rate limit.", app_id);
        return format!("ORE KERNEL ALERT: Rate Limit Exceeded. Quota is {} tokens/min.", limit);
    }

    let secured_prompt = match ContextFirewall::secure_request(manifest, &payload.prompt) {
        Ok(safe_text) => {
            println!("-> Security Check Passed.");
            safe_text
        }
        Err(e) => {
            println!("-> [BLOCKED] {}", e);
            return format!("ORE KERNEL ALERT: {}", e);
        }
    };

    println!("-> Waiting for GPU Scheduler...");

    // request a GPU lease for the specified model
    let lease = state.scheduler.request_gpu(&payload.model).await;
    println!("-> GPU Lease Granted for '{}'. Executing...", lease.model);

    let client = reqwest::Client::new();
    let request_body = OllamaRequest {
        model: lease.model.clone(),
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
        Ok(response) => match response.json::<OllamaResponse>().await {
            Ok(json) => {
                println!("-> Execution complete. Releasing GPU Lock.");
                println!("========================================");
                json.response
            }
            Err(_) => "Kernel Error: Failed to parse AI response.".to_string(),
        },
        Err(_) => "Kernel Error: Driver is offline.".to_string(),
    }
}

async fn process_status() -> String {
    let driver = OllamaDriver::new("http://127.0.0.1:11434");

    match driver.get_running_models().await {
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
    println!(
        "-> [KERNEL COMMAND] Received SIGKILL for model '{}'.",
        model_name
    );

    let driver = OllamaDriver::new("http://127.0.0.1:11434");

    match driver.unload_model(&model_name).await {
        Ok(_) => {
            println!("-> [SUCCESS] VRAM flushed. Model '{}' evicted.", model_name);
            format!(
                "SUCCESS: Model '{}' has been forcefully evicted from GPU VRAM.",
                model_name
            )
        }
        Err(e) => {
            println!("-> [ERROR] Failed to flush VRAM: {}", e);
            format!("KERNEL ERROR: Could not evict model. {}", e)
        }
    }
}

async fn pull_model(Path(model_name): Path<String>) -> String {
    println!(
        "-> [PACKAGE MANAGER] Instructing driver to install model '{}'...",
        model_name
    );

    let driver = OllamaDriver::new("http://127.0.0.1:11434");

    match driver.pull_model(&model_name).await {
        Ok(_) => {
            println!(
                "-> [SUCCESS] Model '{}' successfully installed to local hardware.",
                model_name
            );
            format!(
                "SUCCESS: Model '{}' installed and ready for inference.",
                model_name
            )
        }
        Err(e) => {
            println!("-> [ERROR] Installation failed: {}", e);
            format!("KERNEL ERROR: Could not install model. {}", e)
        }
    }
}

async fn load_model(Path(model_name): Path<String>) -> String {
    println!(
        "-> [KERNEL COMMAND] Allocating VRAM and pre-loading '{}'...",
        model_name
    );

    let driver = OllamaDriver::new("http://127.0.0.1:11434");

    match driver.preload_model(&model_name).await {
        Ok(_) => {
            println!("-> [SUCCESS] Model '{}' locked into VRAM.", model_name);
            format!(
                "SUCCESS: Model '{}' is now pre-loaded and ready for zero-latency inference.",
                model_name
            )
        }
        Err(e) => {
            println!("-> [ERROR] Failed to allocate VRAM: {}", e);
            format!("KERNEL ERROR: Could not load model. {}", e)
        }
    }
}

async fn list_agents(State(state): State<Arc<KernelState>>) -> String {
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
                format!("{}...", &models[..17])
            } else {
                models
            };

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
                app.app_id, app.version, models_disp, priority, status
            ));
        }
    }
    output
}

async fn list_manifests(State(state): State<Arc<KernelState>>) -> String {
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

const SYSTEM_EMBEDDER: &str = "nomic-embed-text";

async fn sys_share_context(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<IpcShareRequest>,
) -> String {

    let manifest = match state.registry.get_app(&payload.source_app) {
        Some(m) => m,
        None => {
            println!("->[SECURITY ALERT] Ghost Agent '{}' tried to write to memory!", payload.source_app);
            return format!("KERNEL ALERT: Unregistered Agent '{}'. Access Denied.", payload.source_app);
        }
    };

    if !manifest.ipc.allowed_ipc_targets.contains(&payload.target_pipe) {
        println!("-> [BLOCKED] Agent '{}' tried to write to restricted pipe '{}'.", payload.source_app, payload.target_pipe);
        return format!("KERNEL ALERT: Permission Denied. Add '{}' to allowed_ipc_targets in manifest.", payload.target_pipe);
    }

    println!("-> [SEMANTIC BUS] Verified Agent '{}' is uploading data to pipe '{}'", manifest.app_id, payload.target_pipe);
    
    let driver = OllamaDriver::new(&state.ollama_url);
    
    // Chunking Algorithm (Splits large text into 100-word blocks)
    let words: Vec<&str> = payload.knowledge_text.split_whitespace().collect();
    let chunks: Vec<String> = words.chunks(100).map(|c| c.join(" ")).collect();
    
    println!("-> [SEMANTIC BUS] Text chunked into {} blocks. Waking up CPU Embedder...", chunks.len());

    // Convert text to Math Vectors
    for chunk in chunks {
        match driver.generate_embeddings(SYSTEM_EMBEDDER, &chunk).await {
            Ok(vector) => {
                state.semantic_bus.write_chunk(&payload.target_pipe, chunk, vector);
            }
            Err(e) => return format!("KERNEL ERROR: Failed to embed knowledge. {}", e),
        }
    }

    // ZERO-RAM ARCHITECTURE: kill the Nomic model to free memory
    let _ = driver.unload_model(SYSTEM_EMBEDDER).await;
    
    println!("-> [SEMANTIC BUS] Knowledge embedded. CPU memory flushed (0MB Idle).");
    "SUCCESS: Knowledge processed and stored in Semantic Bus.".to_string()
}

async fn sys_search_context(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<IpcSearchRequest>,
) -> axum::response::Json<Vec<String>> {
    
    let manifest = match state.registry.get_app(&payload.source_app) {
        Some(m) => m,
        None => {
            println!("-> [SECURITY ALERT] Ghost Agent '{}' tried to read memory!", payload.source_app);
            return axum::response::Json(vec![format!("KERNEL ALERT: Unregistered Agent '{}'.", payload.source_app)]);
        }
    };

    if !manifest.ipc.allowed_ipc_targets.contains(&payload.target_pipe) {
        println!("-> [BLOCKED] Agent '{}' tried to read restricted pipe '{}'.", payload.source_app, payload.target_pipe);
        return axum::response::Json(vec![format!("KERNEL ALERT: Permission Denied. Pipe '{}' is locked.", payload.target_pipe)]);
    }

    println!("-> [SEMANTIC BUS] Verified Agent '{}' searching pipe '{}' for: {}", manifest.app_id, payload.target_pipe, payload.query);

    let driver = OllamaDriver::new(&state.ollama_url);
    
    // Translate the question into Math using the System Embedder
    let query_vector = match driver.generate_embeddings(SYSTEM_EMBEDDER, &payload.query).await {
        Ok(v) => v,
        Err(_) => return axum::response::Json(vec!["KERNEL ERROR: Embedding failed.".to_string()]),
    };

    // Perform Pure-Rust Math Search (Zero GPU used here)
    let top_results = state.semantic_bus.search_pipe(&payload.target_pipe, &query_vector, 3); // Get Top 3 matches

    let _ = driver.unload_model(SYSTEM_EMBEDDER).await;

    println!("-> [SEMANTIC BUS] Search complete. Handing English text back to Agent.");
    
    axum::response::Json(top_results)
}

async fn ipc_send(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<AgentMessage>,
) -> String {
    println!("-> [IPC BUS] Routing message from '{}' to '{}'", payload.from_app, payload.to_app);
    
    // ORE IPC FIREWALL: Is the sender registered?
    let manifest = match state.registry.get_app(&payload.from_app) {
        Some(m) => m,
        None => return format!("KERNEL ERROR: Unregistered sender '{}'.", payload.from_app),
    };

    // ORE IPC FIREWALL: Is the sender allowed to talk to the target?
    if !manifest.ipc.allowed_ipc_targets.contains(&payload.to_app) {
        println!("-> [BLOCKED] '{}' is not authorized by its manifest to contact '{}'.", payload.from_app, payload.to_app);
        return format!("KERNEL ALERT: IPC Target '{}' not in allowed_ipc_targets manifest.", payload.to_app);
    }

    // Route the message instantly in RAM
    match state.message_bus.send_message(payload) {
        Ok(_) => {
            println!("-> [SUCCESS] Message delivered to local channel.");
            "SUCCESS: Message delivered.".to_string()
        },
        Err(e) => {
            println!("-> [WARN] {}", e);
            format!("KERNEL ERROR: {}", e)
        },
    }
}

async fn ipc_listen(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> axum::response::Json<Option<AgentMessage>> {
    println!("-> [IPC BUS] App '{}' is polling its channel...", app_id);
    
    let mut receiver = state.message_bus.register_listener(&app_id);
    
    match receiver.try_recv() {
        Ok(msg) => axum::response::Json(Some(msg)),
        Err(_) => axum::response::Json(None),
    }
}

async fn auth_middleware(
    State(state): State<Arc<KernelState>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. Extract the Authorization header
    if let Some(auth_header) = headers.get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str == format!("Bearer {}", state.auth_token) {
                return Ok(next.run(request).await); 
            }
        }
    }
    
    println!("-> [SECURITY ALERT] Blocked unauthorized network connection attempt!");
    Err(StatusCode::UNAUTHORIZED)
}