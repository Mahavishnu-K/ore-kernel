use axum::{extract::{Json, Path, State}, response::Json as JsonResponse};
use std::sync::Arc;
use crate::state::KernelState;
use crate::payloads::{IpcShareRequest, IpcSearchRequest};
use ore_core::ipc::{AgentMessage, SemanticBus};

const SYSTEM_EMBEDDER: &str = "nomic-embed-text";

pub async fn sys_share_context(
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

    if !manifest.ipc.allowed_semantic_pipes.contains(&payload.target_pipe) {
        println!("-> [BLOCKED] Agent '{}' tried to write to restricted pipe '{}'.", payload.source_app, payload.target_pipe);
        return format!("KERNEL ALERT: Permission Denied. Add '{}' to allowed_semantic_pipes in manifest.", payload.target_pipe);
    }

    println!("-> [SEMANTIC BUS] Verified Agent '{}' is uploading data to pipe '{}'", manifest.app_id, payload.target_pipe);
    
    // Chunking Algorithm (Splits large text into 100-word blocks)
    let chunks = SemanticBus::create_sliding_windows(&payload.knowledge_text, 100, 20);
    println!("-> [SEMANTIC BUS] Text split into {} overlapping windows.", chunks.len());
    
    println!("-> [SEMANTIC BUS] Text chunked into {} blocks. Waking up CPU Embedder...", chunks.len());

    let mut wake_embedder = false;

    // Convert text to Math Vectors
    for chunk in chunks {
        if let Some(cached_vector) = state.semantic_bus.get_cached_embedding(&chunk) {
            println!("-> [SEMANTIC BUS] Cache Hit! Skipping math for this chunk.");
            state.semantic_bus.write_chunk(&payload.target_pipe, chunk, cached_vector, &manifest.app_id);
            continue;
        }
        wake_embedder = true;
        match state.driver.generate_embeddings(SYSTEM_EMBEDDER, &chunk).await {
            Ok(vector) => {
                state.semantic_bus.write_chunk(&payload.target_pipe, chunk, vector, &manifest.app_id);
            }
            Err(e) => return format!("KERNEL ERROR: Failed to embed knowledge. {}", e),
        }
    }

    // ZERO-RAM ARCHITECTURE: kill the Nomic model to free memory
    if wake_embedder {
        let _ = state.driver.unload_model(SYSTEM_EMBEDDER).await;
        println!("-> [SEMANTIC BUS] Knowledge embedded. CPU memory flushed (0MB Idle).");
    } else {
        println!("-> [SEMANTIC BUS] Knowledge embedded entirely from Cache. Zero compute used.");
    }

    "SUCCESS: Knowledge processed and stored in Semantic Bus.".to_string()
}

pub async fn sys_search_context(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<IpcSearchRequest>,
) -> JsonResponse<Vec<String>> {
    
    let manifest = match state.registry.get_app(&payload.source_app) {
        Some(m) => m,
        None => {
            println!("-> [SECURITY ALERT] Ghost Agent '{}' tried to read memory!", payload.source_app);
            return JsonResponse(vec![format!("KERNEL ALERT: Unregistered Agent '{}'.", payload.source_app)]);
        }
    };

    if !manifest.ipc.allowed_semantic_pipes.contains(&payload.target_pipe) {
        println!("-> [BLOCKED] Agent '{}' tried to read restricted pipe '{}'.", payload.source_app, payload.target_pipe);
        return JsonResponse(vec![format!("KERNEL ALERT: Permission Denied. Add '{}' to allowed_semantic_pipes in manifest.", payload.target_pipe)]);
    }

    println!("-> [SEMANTIC BUS] Verified Agent '{}' searching pipe '{}' for: {}", manifest.app_id, payload.target_pipe, payload.query);
    
    // Translate the question into Math using the System Embedder
    let query_vector = match state.driver.generate_embeddings(SYSTEM_EMBEDDER, &payload.query).await {
        Ok(v) => v,
        Err(_) => return JsonResponse(vec!["KERNEL ERROR: Embedding failed.".to_string()]),
    };

    // Perform Pure-Rust Math Search (Zero GPU used here)
    let filter_ref = payload.filter_app.as_deref();
    let top_results = state.semantic_bus.search_pipe(&payload.target_pipe, &query_vector, 3, filter_ref); 

    let _ = state.driver.unload_model(SYSTEM_EMBEDDER).await;

    println!("-> [SEMANTIC BUS] Search complete. Handing English text back to Agent.");
    
    JsonResponse(top_results)
}

pub async fn ipc_send(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<AgentMessage>,
) -> String {
    println!("-> [IPC BUS] Routing message from '{}' to '{}'", payload.from_app, payload.to_app);
    
    // ore ipc firewall
    let manifest = match state.registry.get_app(&payload.from_app) {
        Some(m) => m,
        None => return format!("KERNEL ERROR: Unregistered sender '{}'.", payload.from_app),
    };
    if !manifest.ipc.allowed_agent_targets.contains(&payload.to_app) {
        println!("-> [BLOCKED] '{}' is not authorized by its manifest to contact '{}'.", payload.from_app, payload.to_app);
        return format!("KERNEL ALERT: IPC Target '{}' not in allowed_agent_targets manifest.", payload.to_app);
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

pub async fn ipc_listen(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> JsonResponse<Option<AgentMessage>> {

    let _manifest = match state.registry.get_app(&app_id) {
        Some(m) => m,
        None => {
            println!("-> [SECURITY ALERT] Ghost Agent '{}' tried to wiretap a channel!", app_id);
            
            return JsonResponse(None); 
        }
    };

    println!("-> [IPC BUS] App '{}' is polling its channel...", app_id);
    
    let mut receiver = state.message_bus.register_listener(&app_id);
    
    match receiver.try_recv() {
        Ok(msg) => JsonResponse(Some(msg)),
        Err(_) => JsonResponse(None),
    }
}