use crate::payloads::{IpcSearchRequest, IpcShareRequest};
use crate::state::KernelState;
use axum::{
    extract::{Json, Path, State},
    response::Json as JsonResponse,
};
use ore_core::ipc::{AgentMessage, SemanticBus};
use std::sync::Arc;

const SYSTEM_EMBEDDER: &str = "system-embedder";

pub async fn sys_share_context(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<IpcShareRequest>,
) -> String {
    let manifest = match state.registry.get_app(&payload.source_app) {
        Some(m) => m,
        None => {
            println!(
                "->[SECURITY ALERT] Ghost Agent '{}' tried to write to memory!",
                payload.source_app
            );
            return format!(
                "KERNEL ALERT: Unregistered Agent '{}'. Access Denied.",
                payload.source_app
            );
        }
    };

    if !manifest
        .ipc
        .allowed_semantic_pipes
        .contains(&payload.target_pipe)
    {
        println!(
            "-> [BLOCKED] Agent '{}' tried to write to restricted pipe '{}'.",
            payload.source_app, payload.target_pipe
        );
        return format!(
            "KERNEL ALERT: Permission Denied. Add '{}' to allowed_semantic_pipes in manifest.",
            payload.target_pipe
        );
    }

    println!(
        "-> [SEMANTIC BUS] Verified Agent '{}' is uploading data to pipe '{}'",
        manifest.app_id, payload.target_pipe
    );

    // Dynamic Chunking Algorithm
    // Read the agent's request, or fallback to sensible defaults (100 words, 20 overlap)
    let c_size = payload.chunk_size.unwrap_or(100);
    let c_overlap = payload.chunk_overlap.unwrap_or(20);

    let safe_overlap = if c_overlap >= c_size {
        c_size / 4
    } else {
        c_overlap
    };

    let chunks = SemanticBus::create_sliding_windows(&payload.knowledge_text, c_size, safe_overlap);

    let total_blocks = chunks.len();

    if total_blocks == 0 {
        println!("-> [SEMANTIC BUS] [WARN] Input text was empty. Skipping embedding.");
        return "SUCCESS: No content to process.".to_string();
    }

    println!(
        "-> [SEMANTIC BUS] Text split into {} overlapping windows.",
        total_blocks
    );
    println!(
        "-> [SEMANTIC BUS] Ready to process {} blocks. Waking up CPU Embedder...",
        total_blocks
    );

    let mut chunks_to_embed = Vec::new();
    let mut cached_chunks = Vec::new();

    for chunk in chunks.clone() {
        if let Some(cached_vector) = state.semantic_bus.get_cached_embedding(&chunk) {
            cached_chunks.push((chunk, cached_vector));
        } else {
            chunks_to_embed.push(chunk);
        }
    }

    let mut wake_embedder = false;

    // Convert text to Math Vectors
    if !chunks_to_embed.is_empty() {
        wake_embedder = true;

        match state
            .driver
            .generate_embeddings(SYSTEM_EMBEDDER, chunks_to_embed.clone())
            .await
        {
            Ok(vectors) => {
                for (chunk, vector) in chunks_to_embed.into_iter().zip(vectors.into_iter()) {
                    state.semantic_bus.write_chunk(
                        &payload.target_pipe,
                        chunk,
                        vector,
                        &manifest.app_id,
                    );
                }
            }
            Err(e) => return format!("KERNEL ERROR: Failed to embed knowledge. {}", e),
        }
    }

    for (chunk, vector) in cached_chunks {
        state
            .semantic_bus
            .write_chunk(&payload.target_pipe, chunk, vector, &manifest.app_id);
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
            println!(
                "-> [SECURITY ALERT] Ghost Agent '{}' tried to read memory!",
                payload.source_app
            );
            return JsonResponse(vec![format!(
                "KERNEL ALERT: Unregistered Agent '{}'.",
                payload.source_app
            )]);
        }
    };

    if !manifest
        .ipc
        .allowed_semantic_pipes
        .contains(&payload.target_pipe)
    {
        println!(
            "-> [BLOCKED] Agent '{}' tried to read restricted pipe '{}'.",
            payload.source_app, payload.target_pipe
        );
        return JsonResponse(vec![format!(
            "KERNEL ALERT: Permission Denied. Add '{}' to allowed_semantic_pipes in manifest.",
            payload.target_pipe
        )]);
    }

    println!(
        "-> [SEMANTIC BUS] Verified Agent '{}' searching pipe '{}' for: {}",
        manifest.app_id, payload.target_pipe, payload.query
    );

    let k = payload.top_k.unwrap_or(3);

    println!(
        "-> [SEMANTIC BUS] Retrieving the top {} most relevant memory chunks...",
        k
    );

    // Translate the question into Math using the System Embedder
    let query_vector = match state
        .driver
        .generate_embeddings(SYSTEM_EMBEDDER, vec![payload.query.clone()])
        .await
    {
        Ok(v) => v[0].clone(),
        Err(_) => return JsonResponse(vec!["KERNEL ERROR: Embedding failed.".to_string()]),
    };

    // Perform Pure-Rust Math Search (Zero GPU used here)
    let filter_ref = payload.filter_app.as_deref();
    let top_results =
        state
            .semantic_bus
            .search_pipe(&payload.target_pipe, &query_vector, k, filter_ref);

    let _ = state.driver.unload_model(SYSTEM_EMBEDDER).await;

    println!("-> [SEMANTIC BUS] Search complete. Handing English text back to Agent.");

    JsonResponse(top_results)
}

pub async fn ipc_send(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<AgentMessage>,
) -> String {
    println!(
        "-> [IPC BUS] Routing message from '{}' to '{}'",
        payload.from_app, payload.to_app
    );

    // ore ipc firewall
    let manifest = match state.registry.get_app(&payload.from_app) {
        Some(m) => m,
        None => return format!("KERNEL ERROR: Unregistered sender '{}'.", payload.from_app),
    };
    if !manifest.ipc.allowed_agent_targets.contains(&payload.to_app) {
        println!(
            "-> [BLOCKED] '{}' is not authorized by its manifest to contact '{}'.",
            payload.from_app, payload.to_app
        );
        return format!(
            "KERNEL ALERT: IPC Target '{}' not in allowed_agent_targets manifest.",
            payload.to_app
        );
    }

    // Route the message instantly in RAM
    match state.message_bus.send_message(payload) {
        Ok(_) => {
            println!("-> [SUCCESS] Message delivered to local channel.");
            "SUCCESS: Message delivered.".to_string()
        }
        Err(e) => {
            println!("-> [WARN] {}", e);
            format!("KERNEL ERROR: {}", e)
        }
    }
}

pub async fn ipc_listen(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> JsonResponse<Option<AgentMessage>> {
    let _manifest = match state.registry.get_app(&app_id) {
        Some(m) => m,
        None => {
            println!(
                "-> [SECURITY ALERT] Ghost Agent '{}' tried to wiretap a channel!",
                app_id
            );

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
