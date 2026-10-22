use crate::payloads::ChunkStrategy;
use crate::payloads::{
    ExecuteRequest, IpcSearchRequest, IpcShareRequest, PipeBranchRequest, PipeDiffRequest,
    PipeDiffResponse, PipeMergeRequest, SearchResult, StagedChunk,
};
use crate::state::KernelState;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json as JsonResponse},
};
use ore_core::ipc::{AgentMessage, SemanticBus};
use ore_core::kprintln;
use ore_core::memory::Pager;
use std::sync::Arc;

/// Validates pipe access including wildcard branch patterns (e.g., "incident_409:branch/*" or "*")
pub fn is_pipe_permitted(allowed_pipes: &[String], target_pipe: &str) -> bool {
    for pattern in allowed_pipes {
        if pattern == "*" || pattern == target_pipe {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix("/*") {
            if target_pipe.starts_with(prefix) && target_pipe[prefix.len()..].starts_with('/') {
                return true;
            }
        } else if let Some(prefix) = pattern.strip_suffix('*')
            && target_pipe.starts_with(prefix)
        {
            return true;
        }
    }
    false
}

/// POST /ipc/pipe/branch - Fork a base pipe into an isolated Copy-on-Write memory branch
pub async fn sys_branch_pipe(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<PipeBranchRequest>,
) -> impl IntoResponse {
    let manifest = match state.registry.get_app(&payload.app_id) {
        Some(m) => m,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                format!("KERNEL ALERT: Unregistered Agent '{}'.", payload.app_id),
            )
                .into_response();
        }
    };

    if !is_pipe_permitted(&manifest.ipc.allowed_semantic_pipes, &payload.base_pipe) {
        return (
            StatusCode::FORBIDDEN,
            format!(
                "Permission Denied: Base pipe '{}' is restricted.",
                payload.base_pipe
            ),
        )
            .into_response();
    }

    if !is_pipe_permitted(&manifest.ipc.allowed_semantic_pipes, &payload.branch_pipe) {
        return (
            StatusCode::FORBIDDEN,
            format!(
                "Permission Denied: Target branch pipe '{}' is restricted.",
                payload.branch_pipe
            ),
        )
            .into_response();
    }

    match state
        .semantic_bus
        .branch_pipe(&payload.base_pipe, &payload.branch_pipe)
    {
        Ok(_) => (
            StatusCode::OK,
            format!(
                "SUCCESS: Forked branch '{}' from base '{}' (Zero-copy CoW).",
                payload.branch_pipe, payload.base_pipe
            ),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("KERNEL ERROR: {}", e)).into_response(),
    }
}

/// POST /ipc/pipe/diff - View unmerged staged chunks in a memory branch
pub async fn sys_diff_pipe(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<PipeDiffRequest>,
) -> impl IntoResponse {
    let manifest = match state.registry.get_app(&payload.app_id) {
        Some(m) => m,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                format!("KERNEL ALERT: Unregistered Agent '{}'.", payload.app_id),
            )
                .into_response();
        }
    };

    if !is_pipe_permitted(&manifest.ipc.allowed_semantic_pipes, &payload.branch_pipe) {
        return (
            StatusCode::FORBIDDEN,
            format!(
                "Permission Denied: Branch '{}' is locked.",
                payload.branch_pipe
            ),
        )
            .into_response();
    }

    match state
        .semantic_bus
        .diff_pipe(&payload.base_pipe, &payload.branch_pipe)
    {
        Ok(chunks) => {
            let staged_chunks: Vec<StagedChunk> = chunks
                .into_iter()
                .map(|c| StagedChunk {
                    id: c.id,
                    text: c.text.to_string(),
                    context_payload: c.context_payload.to_string(),
                    source_app: c.source_app.clone(),
                    timestamp: c.timestamp,
                    category: c.category,
                })
                .collect();

            let resp = PipeDiffResponse {
                base_pipe: payload.base_pipe,
                branch_pipe: payload.branch_pipe,
                delta_count: staged_chunks.len(),
                staged_chunks,
            };
            JsonResponse(resp).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("KERNEL ERROR: {}", e)).into_response(),
    }
}

/// POST /ipc/pipe/merge - Atomically commit branch chunks into base pipe and flush to SSD
pub async fn sys_merge_pipe(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<PipeMergeRequest>,
) -> impl IntoResponse {
    let manifest = match state.registry.get_app(&payload.app_id) {
        Some(m) => m,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                format!("KERNEL ALERT: Unregistered Agent '{}'.", payload.app_id),
            )
                .into_response();
        }
    };

    if !is_pipe_permitted(&manifest.ipc.allowed_semantic_pipes, &payload.base_pipe) {
        return (
            StatusCode::FORBIDDEN,
            format!(
                "Permission Denied: Target base pipe '{}' is locked.",
                payload.base_pipe
            ),
        )
            .into_response();
    }

    match state.semantic_bus.merge_pipe(
        &payload.base_pipe,
        &payload.branch_pipe,
        payload.delete_branch_after_merge,
    ) {
        Ok(count) => (
            StatusCode::OK,
            format!(
                "SUCCESS: Merged {} chunks from '{}' into '{}' (Crash-consistent SSD commit).",
                count, payload.branch_pipe, payload.base_pipe
            ),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("KERNEL ERROR: {}", e)).into_response(),
    }
}

pub async fn sys_share_context(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<IpcShareRequest>,
) -> String {
    let manifest = match state.registry.get_app(&payload.source_app) {
        Some(m) => m,
        None => {
            crate::kprintln!(
                "->[SECURITY ALERT] Ghost Agent '{}' tried to write to memory!",
                payload.source_app
            );
            return format!(
                "KERNEL ALERT: Unregistered Agent '{}'. Access Denied.",
                payload.source_app
            );
        }
    };

    if !is_pipe_permitted(&manifest.ipc.allowed_semantic_pipes, &payload.target_pipe) {
        kprintln!(
            "-> [BLOCKED] Agent '{}' tried to write to restricted pipe '{}'.",
            payload.source_app,
            payload.target_pipe
        );
        return format!(
            "KERNEL ALERT: Permission Denied. Add '{}' to allowed_semantic_pipes in manifest.",
            payload.target_pipe
        );
    }

    kprintln!(
        "-> [SEMANTIC BUS] Verified Agent '{}' is uploading data to pipe '{}'",
        manifest.app_id,
        payload.target_pipe
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

    let strategy_str = match payload.chunk_strategy.unwrap_or_default() {
        ChunkStrategy::SlidingWindow => "sliding_window",
        ChunkStrategy::SentenceAware => "sentence_aware",
        ChunkStrategy::SentenceWindow => "sentence_window",
        ChunkStrategy::Paragraph => "paragraph",
        ChunkStrategy::ExactMatch => "exact_match",
    };

    let chunks =
        SemanticBus::chunk_text(&payload.knowledge_text, strategy_str, c_size, safe_overlap);

    let total_blocks = chunks.len();

    if total_blocks == 0 {
        kprintln!("-> [SEMANTIC BUS] [WARN] Input text was empty. Skipping embedding.");
        return "SUCCESS: No content to process.".to_string();
    }

    kprintln!(
        "-> [SEMANTIC BUS] Ready to process {} blocks. Waking up CPU Embedder...",
        total_blocks
    );

    kprintln!(
        "-> [SEMANTIC BUS] Decoupled representation prepared: {} chunks (Strategy: '{}').",
        total_blocks,
        strategy_str
    );

    let mut chunks_to_embed = Vec::new();
    let mut cached_chunks = Vec::new();

    for chunk in chunks.clone() {
        if let Some(cached_vector) = state.semantic_bus.get_cached_embedding(&chunk.probe_text) {
            cached_chunks.push((chunk, cached_vector));
        } else {
            chunks_to_embed.push(chunk);
        }
    }

    let mut wake_embedder = false;

    let category = payload
        .category
        .unwrap_or(ore_common::protocol::MemoryCategory::General);

    let _embedder_guard = state.embedder_lock.lock().await;

    // Convert text to Math Vectors
    if !chunks_to_embed.is_empty() {
        wake_embedder = true;
        let probe_texts: Vec<String> = chunks_to_embed
            .iter()
            .map(|c| c.probe_text.clone())
            .collect();

        match state
            .driver
            .generate_embeddings(&state.system_embedder, probe_texts)
            .await
        {
            Ok(vectors) => {
                for (chunk_payload, vector) in chunks_to_embed.into_iter().zip(vectors.into_iter())
                {
                    state.semantic_bus.write_chunk(
                        &payload.target_pipe,
                        chunk_payload.probe_text,
                        chunk_payload.context_payload,
                        vector,
                        &manifest.app_id,
                        manifest.ipc.semantic_persistence,
                        category,
                    );
                }
            }
            Err(e) => return format!("KERNEL ERROR: Failed to embed knowledge. {}", e),
        }
    }

    for (chunk_payload, arc_vector) in cached_chunks {
        state.semantic_bus.write_cached_chunk(
            &payload.target_pipe,
            chunk_payload.probe_text,
            chunk_payload.context_payload,
            arc_vector,
            &manifest.app_id,
            manifest.ipc.semantic_persistence,
            category,
        );
    }

    // ZERO-RAM ARCHITECTURE: kill the Nomic model to free memory
    if wake_embedder {
        let _ = state.driver.unload_model(&state.system_embedder).await;
        kprintln!("-> [SEMANTIC BUS] Knowledge embedded. CPU memory flushed (0MB Idle).");
    } else {
        kprintln!("-> [SEMANTIC BUS] Knowledge embedded entirely from Cache. Zero compute used.");
    }

    if manifest.ipc.semantic_persistence {
        let p_name = payload.target_pipe.clone();

        // Grab the updated pipe from RAM
        if let Some(pipe_contents) = state.semantic_bus.get_pipe_contents(&p_name) {
            // Spawn a background thread to freeze it to the SSD without blocking the API!
            tokio::spawn(async move {
                Pager::page_out_semantic(&p_name, &pipe_contents);
            });
        }
    } else {
        kprintln!(
            "-> [SEMANTIC BUS] Ephemeral Mode: Data secured in RAM only. (semantic_persistence = false)"
        );
    }

    "SUCCESS: Knowledge processed and stored in Semantic Bus.".to_string()
}

pub async fn sys_search_context(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<IpcSearchRequest>,
) -> impl IntoResponse {
    // 1. REGISTRY CHECK
    let manifest = match state.registry.get_app(&payload.source_app) {
        Some(m) => m,
        None => {
            kprintln!(
                "-> [SECURITY ALERT] Ghost Agent '{}' tried to read memory!",
                payload.source_app
            );
            return (
                StatusCode::UNAUTHORIZED,
                format!("KERNEL ALERT: Unregistered Agent '{}'.", payload.source_app),
            )
                .into_response();
        }
    };

    // PERMISSION CHECK
    if !is_pipe_permitted(&manifest.ipc.allowed_semantic_pipes, &payload.target_pipe) {
        kprintln!(
            "-> [BLOCKED] Agent '{}' tried to read restricted pipe '{}'.",
            payload.source_app,
            payload.target_pipe
        );
        return (
            StatusCode::FORBIDDEN,
            format!(
                "KERNEL ALERT: Permission Denied. Pipe '{}' is locked.",
                payload.target_pipe
            ),
        )
            .into_response();
    }

    kprintln!(
        "-> [SEMANTIC BUS] Verified Agent '{}' searching pipe '{}'",
        manifest.app_id,
        payload.target_pipe
    );

    // AUTONOMOUS HyDE (Hypothetical Document Embeddings) PRE-PASS
    let enable_hyde = payload.enable_hyde.unwrap_or(manifest.rag.enable_hyde);

    let dense_query_text = if enable_hyde {
        kprintln!(
            "-> [HyDE] Synthesizing hypothetical document for query: '{}'...",
            payload.query
        );
        let hyde_prompt = format!(
            "Write a factual, one-sentence direct answer excerpt to this question. Output ONLY the answer excerpt, no chat:\nQuestion: {}\nHypothetical Excerpt:",
            payload.query
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let driver = Arc::clone(&state.driver);
        let target_model = manifest
            .resources
            .allowed_models
            .first()
            .map(|s| s.as_str())
            .unwrap_or("llama3.2:1b")
            .to_string();
        let app_id = manifest.app_id.clone();

        tokio::spawn(async move {
            let _ = driver
                .generate_text(&target_model, &app_id, false, &hyde_prompt, None, tx, "")
                .await;
        });

        let mut hypo = String::new();
        while let Some(word) = rx.recv().await {
            hypo.push_str(&word);
        }
        let trimmed = hypo.trim().to_string();
        if trimmed.is_empty() {
            payload.query.clone()
        } else {
            kprintln!("-> [HyDE] Synthesized Excerpt: '{}'", trimmed);
            trimmed
        }
    } else {
        payload.query.clone()
    };

    // Generate Embeddings (computed on HyDE text for dense search)
    let query_vector =
        if let Some(cached_vec) = state.semantic_bus.get_cached_embedding(&dense_query_text) {
            kprintln!("-> [SEMANTIC BUS] Query found in System Cache. Zero compute used.");
            cached_vec
        } else {
            let _embedder_guard = state.embedder_lock.lock().await;

            match state
                .driver
                .generate_embeddings(&state.system_embedder, vec![dense_query_text.clone()])
                .await
            {
                Ok(v) => {
                    let arc_vec = std::sync::Arc::new(v[0].clone());

                    // Use cache_only instead of write_chunk!
                    // This stores the math for the Kernel's eyes only.
                    state
                        .semantic_bus
                        .cache_only(&dense_query_text, Arc::clone(&arc_vec));

                    arc_vec
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("KERNEL ERROR: Failed to generate query embedding: {}", e),
                    )
                        .into_response();
                }
            }
        };

    let k = payload.top_k.unwrap_or(5);
    let filter_ref = payload.filter_app.as_deref();
    let filter_cat = payload.filter_category;
    let enable_hybrid = payload.enable_hybrid.unwrap_or(manifest.rag.enable_hybrid);
    let confidence_threshold = payload
        .confidence_threshold
        .unwrap_or(manifest.rag.confidence_threshold);
    let crag_fallback_tool = payload
        .crag_fallback_tool
        .clone()
        .or_else(|| manifest.rag.crag_fallback_tool.clone());

    kprintln!(
        "-> [SEMANTIC BUS] Executing Hybrid Search (BM25 + Dense SIMD RRF, top_k={})...",
        k
    );

    let mut outcome = state.semantic_bus.search_pipe(
        &payload.target_pipe,
        &query_vector,
        &payload.query,
        k,
        filter_ref,
        filter_cat,
        manifest.ipc.allow_time_decay,
        enable_hybrid,
    );

    // CRAG CONFIDENCE GATE & FALLBACK EXECUTION
    let mut crag_rescued = false;
    if outcome.top_dense_score < confidence_threshold
        && let Some(tool_name) = crag_fallback_tool.as_ref()
    {
        kprintln!(
            "-> [CRAG] Low confidence retrieval ({:.2} < {:.2}). Auto-invoking fallback WASM tool '{}'...",
            outcome.top_dense_score,
            confidence_threshold,
            tool_name
        );

        let exec_req = ExecuteRequest {
            app_id: manifest.app_id.clone(),
            tool_name: Some(tool_name.clone()),
            // prepare_tool already sets argv[0] to the tool name; the query is argv[1].
            args: Some(vec![payload.query.clone()]),
            input_data: Some(payload.query.clone()),
            language: None,
            script: None,
            dependencies: None,
            shell_command: None,
        };

        match crate::execution::execute_request(state.clone(), exec_req).await {
            Err(e) => kprintln!(
                "-> [CRAG] Fallback tool '{}' was rejected: {}",
                tool_name,
                e
            ),
            // A trap, non-zero exit, or fuel exhaustion ends the output with a KERNEL ERROR
            // line. Never ingest that as knowledge.
            Ok(output) if output.contains("KERNEL ERROR") => kprintln!(
                "-> [CRAG] Fallback tool '{}' failed; nothing ingested.",
                tool_name
            ),
            Ok(live_output) => {
                let trimmed = live_output.trim();
                if !trimmed.is_empty() {
                    kprintln!(
                        "-> [CRAG] Fallback tool returned findings. Ingesting live context into pipe..."
                    );

                    let _embedder_guard = state.embedder_lock.lock().await;
                    if let Ok(vecs) = state
                        .driver
                        .generate_embeddings(&state.system_embedder, vec![trimmed.to_string()])
                        .await
                        && let Some(live_vec) = vecs.into_iter().next()
                    {
                        state.semantic_bus.write_chunk(
                            &payload.target_pipe,
                            trimmed.to_string(),
                            trimmed.to_string(),
                            live_vec,
                            tool_name,
                            manifest.ipc.semantic_persistence,
                            ore_common::protocol::MemoryCategory::Fact,
                        );

                        // Re-run search to surface the live finding
                        outcome = state.semantic_bus.search_pipe(
                            &payload.target_pipe,
                            &query_vector,
                            &payload.query,
                            k,
                            filter_ref,
                            filter_cat,
                            manifest.ipc.allow_time_decay,
                            enable_hybrid,
                        );
                        crag_rescued = true;
                    }
                }
            }
        }
    }

    let _ = state.driver.unload_model(&state.system_embedder).await;

    let fallback_tool_name = crag_fallback_tool;

    let results: Vec<SearchResult> = outcome
        .results
        .into_iter()
        .map(|(score, chunk_arc)| {
            let is_from_fallback = crag_rescued
                && (fallback_tool_name
                    .as_deref()
                    .is_some_and(|name| chunk_arc.source_app == name)
                    || chunk_arc.source_app.ends_with(".wasm"));

            SearchResult {
                text: chunk_arc.text.to_string(), // Deref Arc to String
                context_payload: chunk_arc.context_payload.to_string(),
                score,
                source_app: chunk_arc.source_app.clone(),
                timestamp: chunk_arc.timestamp,
                category: chunk_arc.category,
                is_crag_fallback: is_from_fallback,
            }
        })
        .collect();

    kprintln!(
        "-> [SEMANTIC BUS] Search complete. Returning {} results.",
        results.len()
    );

    JsonResponse(results).into_response()
}

pub async fn ipc_send(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<AgentMessage>,
) -> String {
    kprintln!(
        "-> [IPC BUS] Routing message from '{}' to '{}'",
        payload.from_app,
        payload.to_app
    );

    // ore ipc firewall
    let manifest = match state.registry.get_app(&payload.from_app) {
        Some(m) => m,
        None => return format!("KERNEL ERROR: Unregistered sender '{}'.", payload.from_app),
    };
    if !manifest.ipc.allowed_agent_targets.contains(&payload.to_app) {
        kprintln!(
            "-> [BLOCKED] '{}' is not authorized by its manifest to contact '{}'.",
            payload.from_app,
            payload.to_app
        );
        return format!(
            "KERNEL ALERT: IPC Target '{}' not in allowed_agent_targets manifest.",
            payload.to_app
        );
    }

    // Route the message instantly in RAM
    match state.message_bus.send_message(payload) {
        Ok(_) => {
            kprintln!("-> [SUCCESS] Message delivered to local channel.");
            "SUCCESS: Message delivered.".to_string()
        }
        Err(e) => {
            kprintln!("-> [WARN] {}", e);
            format!("KERNEL ERROR: {}", e).to_string()
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
            kprintln!(
                "-> [SECURITY ALERT] Ghost Agent '{}' tried to wiretap a channel!",
                app_id
            );

            return JsonResponse(None);
        }
    };

    kprintln!("-> [IPC BUS] App '{}' is polling its channel...", app_id);

    let receiver = state.message_bus.read_message(&app_id);

    JsonResponse(receiver)
}
