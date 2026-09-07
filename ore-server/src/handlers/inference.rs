use crate::payloads::RunRequest;
use crate::state::KernelState;
use axum::body::Body;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ore_core::firewall::ContextFirewall;
use ore_core::kprintln;
use ore_core::memory::Pager;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;

// inference engine (The Proxy & Firewall)
pub async fn ask_ai(State(state): State<Arc<KernelState>>, Path(prompt): Path<String>) -> String {
    let clean_prompt = prompt.replace("_", " ");

    kprintln!("\n-> Incoming App Request: {}", clean_prompt);

    let app_id = "openclaw"; // In the future, this comes from an API Key/Token
    let manifest = match state.registry.get_app(app_id) {
        Some(m) => m.clone(),
        None => return format!("ORE KERNEL ALERT: Unregistered Agent '{}'.", app_id),
    };

    let (_safe_user_prompt, secured_gpu_prompt) =
        match ContextFirewall::secure_request(&manifest, &clean_prompt) {
            Ok((safe_text, wrapped_text)) => {
                crate::kprintln!("-> Security Check Passed.");
                if safe_text != clean_prompt {
                    crate::kprintln!("-> [NOTICE] PII Redacted from prompt.");
                }
                (safe_text, wrapped_text)
            }
            Err(e) => {
                crate::kprintln!("-> [BLOCKED] {}", e);
                return format!("ORE KERNEL ALERT: {}", e);
            }
        };

    kprintln!("-> Waiting for GPU Scheduler...");

    // If the agent lists allowed_models, pick the first one. Default to "llama3.2:1b"
    let target_model = manifest
        .resources
        .allowed_models
        .first()
        .map(|s| s.as_str())
        .unwrap_or("llama3.2:1b");

    // the GPU scheduler
    let lease = state.scheduler.request_gpu(target_model, app_id).await;
    kprintln!(
        "-> GPU Lease Granted for '{}'. Routing to Driver...",
        lease.model
    );

    let current_fingerprint = Pager::get_history_fingerprint(app_id);
    let mut current_context = None;
    if manifest.resources.json_history {
        current_context = Some(Pager::page_in_history(app_id));
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let driver = Arc::clone(&state.driver);
    let model_name = lease.model.clone();
    let prompt_clone = secured_gpu_prompt.clone();
    let context_clone = current_context.clone();

    tokio::spawn(async move {
        // production update required: app_id -> &manifest.app_id on function signature
        if let Err(e) = driver
            .generate_text(
                &model_name,
                app_id,
                manifest.resources.stateful_paging,
                &prompt_clone,
                context_clone,
                tx,
                &current_fingerprint,
            )
            .await
        {
            kprintln!("-> [KERNEL ERROR] Inference execution failed: {}", e);
        }
    });

    let mut full_response = String::new();
    while let Some(word) = rx.recv().await {
        full_response.push_str(&word);
    }

    if manifest.resources.json_history {
        let mut new_history = current_context.unwrap_or_default();
        new_history.push(ore_core::memory::ContextMessage {
            role: "user".to_string(),
            content: secured_gpu_prompt.clone(),
        });
        new_history.push(ore_core::memory::ContextMessage {
            role: "assistant".to_string(),
            content: full_response.clone(),
        });

        // CRITICAL: write the updated JSON to the SSD before you drop the GpuLease.
        Pager::page_out_history(app_id, &new_history);

        let total_chars: usize = new_history.iter().map(|m| m.content.len()).sum();
        let estimated_tokens = (total_chars / 4) as u32;
        let token_limit = manifest.memory_limits.max_json_tokens;
        let token_cap_hit = estimated_tokens > token_limit;

        // CALCULATE PHYSICAL VRAM/SSD USAGE
        let mut kv_cap_hit = false;
        let mut current_kv_mb = 0;
        if manifest.resources.stateful_paging && manifest.memory_limits.max_kv_cache_mb > 0 {
            current_kv_mb = Pager::get_kv_cache_size_mb(app_id, target_model);
            kv_cap_hit = current_kv_mb > manifest.memory_limits.max_kv_cache_mb;
        }

        if token_cap_hit || kv_cap_hit {
            if manifest.memory_limits.auto_summarize_on_cap {
                kprintln!("-> Agent Execution complete. Releasing GPU Lock for compaction.");
                drop(lease);

                if kv_cap_hit {
                    kprintln!(
                        "-> [KERNEL] Agent '{}' KV cache cap reached ({} > {} MB). Triggering Background Compaction...",
                        app_id,
                        current_kv_mb,
                        manifest.memory_limits.max_kv_cache_mb
                    );
                } else {
                    kprintln!(
                        "-> [KERNEL] Agent '{}' memory cap reached ({} > {} tokens). Triggering Background Compaction...",
                        app_id,
                        estimated_tokens,
                        token_limit
                    );
                }

                // Clone variables for the background thread so the user gets their response instantly
                let history_to_compress = new_history.clone();
                let m_id = app_id.to_string();
                let driver_clone = Arc::clone(&state.driver);
                let scheduler_clone = Arc::clone(&state.scheduler);
                let model_to_use = target_model.to_string();

                tokio::spawn(async move {
                    let text_to_summarize = history_to_compress
                        .iter()
                        .map(|m| format!("{}: {}", m.role, m.content))
                        .collect::<Vec<String>>()
                        .join("\n");

                    let summary_prompt = format!(
                        "You are a system memory compressor. Extract all factual information, user preferences, names, numbers, and core context from the following raw conversation log. Output ONLY a dense bulleted list of facts. Do not converse. Be crisp and concise:\n\n{}",
                        text_to_summarize
                    );

                    // Grab the GPU Lock to do the heavy compression
                    let comp_lease = scheduler_clone.request_gpu(&model_to_use, app_id).await;
                    kprintln!("-> [COMPACTION] GPU Lease acquired for background summarization.");

                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                    let inner_driver = Arc::clone(&driver_clone);
                    let inner_model = model_to_use.clone();
                    let inner_app_id = m_id.clone();

                    tokio::spawn(async move {
                        // We set stateful_paging = false here so the summarizer does not enter an infinite loop!
                        let _ = inner_driver
                            .generate_text(
                                &inner_model,
                                &inner_app_id,
                                false,
                                &summary_prompt,
                                None,
                                tx,
                                "",
                            )
                            .await;
                    });

                    let mut summary = String::new();
                    while let Some(word) = rx.recv().await {
                        summary.push_str(&word);
                    }

                    drop(comp_lease); // Release the GPU

                    // OS SAFETY HEURISTIC 1: TRUNCATE BLOATED SUMMARIES
                    let safe_target = (token_limit as f32 * 0.75) as usize;
                    let max_summary_chars = safe_target * 4;

                    if summary.len() > max_summary_chars {
                        kprintln!(
                            "-> [COMPACTION] [WARN] AI generated a bloated summary. Truncating mechanically."
                        );
                        kprintln!(
                            "-> [COMPACTION] ACTION REQUIRED: Increase 'max_json_tokens' in the Agent's manifest!"
                        );

                        // Safely slice the string at a valid UTF-8 character boundary
                        let safe_idx = summary
                            .char_indices()
                            .nth(max_summary_chars)
                            .map(|(i, _)| i)
                            .unwrap_or(summary.len());
                        summary = format!(
                            "{}... [TRUNCATED BY ORE KERNEL - INCREASE MEMORY LIMITS]",
                            &summary[..safe_idx]
                        );
                    }

                    // REWRITE THE BRAIN
                    let mut compacted_history = Vec::new();
                    compacted_history.push(ore_core::memory::ContextMessage {
                        role: "system".to_string(),
                        content: format!(
                            "You are a helpful AI assistant. Previous context summary: {}",
                            summary.trim()
                        ),
                    });

                    // Keep the last 2 messages so the conversation flow isn't jarring to the user
                    if history_to_compress.len() >= 2 {
                        let len = history_to_compress.len();
                        compacted_history.push(history_to_compress[len - 2].clone());
                        compacted_history.push(history_to_compress[len - 1].clone());
                    }

                    // OS SAFETY HEURISTIC 2: PREVENT COMPACTION DEATH LOOP
                    let new_estimated_tokens = (compacted_history
                        .iter()
                        .map(|m| m.content.len())
                        .sum::<usize>()
                        / 4) as u32;

                    if new_estimated_tokens > safe_target as u32 {
                        kprintln!(
                            "-> [COMPACTION] [WARN] AI failed to compress below limit. Forcing brutal FIFO pruning."
                        );
                        while compacted_history
                            .iter()
                            .map(|m| m.content.len())
                            .sum::<usize>()
                            / 4
                            > safe_target
                            && compacted_history.len() > 1
                        {
                            compacted_history.remove(1);
                        }
                    }

                    // Save the tiny, highly-dense memory back to the SSD
                    Pager::page_out_history(&m_id, &compacted_history);

                    // CRITICAL: We must delete the old .safetensors KV-cache because the sequence of tokens just fundamentally changed!
                    if manifest.resources.stateful_paging {
                        Pager::delete_kv_cache(&m_id);
                        let _ = driver_clone.invalidate_agent_cache(&m_id).await;
                        kprintln!("-> [COMPACTION] KV-Cache invalidated and erased from disk.");
                    }
                    kprintln!(
                        "-> [COMPACTION] Memory compressed successfully. VRAM footprint reset to 0."
                    );
                });
            } else {
                // If auto_summarize is OFF, we use brutal FIFO pruning
                kprintln!(
                    "-> [KERNEL] Agent '{}' memory cap reached. Pruning oldest messages (FIFO)...",
                    app_id
                );
                while new_history.iter().map(|m| m.content.len()).sum::<usize>() / 4
                    > token_limit as usize
                    && new_history.len() > 2
                {
                    new_history.remove(1);
                }
                Pager::page_out_history(app_id, &new_history);
                if manifest.resources.stateful_paging {
                    Pager::delete_kv_cache(app_id);
                    let _ = state.driver.invalidate_agent_cache(app_id).await;
                }
                kprintln!("-> Execution complete. Releasing GPU Lock.");
                drop(lease);
            }
        } else {
            kprintln!("-> Execution complete. Releasing GPU Lock.");
            drop(lease);
        }
    } else {
        kprintln!("-> Execution complete. Releasing GPU Lock.");
        drop(lease);
    }

    full_response
}

pub async fn run_process(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<RunRequest>,
) -> Response {
    let app_id_owned = payload.app_id.clone();

    let app_id = app_id_owned.as_str();

    crate::kprintln!(
        "-> [EXEC] App ID: {} | Model: {} | Prompt: {}",
        app_id,
        payload.model,
        payload.prompt
    );

    let manifest = match state.registry.get_app(app_id) {
        Some(m) => m.clone(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                format!("ORE KERNEL ALERT: Unregistered User '{}'.", app_id),
            )
                .into_response();
        }
    };

    // future update: calculate tokens based on prompt length or use a more dynamic approach
    let limit = manifest.resources.max_tokens_per_minute;
    if !state.rate_limiter.check_and_add(app_id, limit, 1000) {
        kprintln!("-> [BLOCKED] Agent '{}' exceeded GPU rate limit.", app_id);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            format!("ORE KERNEL ALERT: Rate Limit Exceeded ({} t/min).", limit),
        )
            .into_response();
    }

    let (_safe_user_prompt, secured_gpu_prompt) =
        match ContextFirewall::secure_request(&manifest, &payload.prompt) {
            Ok((safe_text, wrapped_text)) => {
                kprintln!("-> Security Check Passed.");
                (safe_text, wrapped_text)
            }
            Err(e) => {
                kprintln!("-> [BLOCKED] {}", e);
                return (StatusCode::FORBIDDEN, format!("ORE KERNEL ALERT: {}", e)).into_response();
            }
        };

    kprintln!("-> Waiting for GPU Scheduler...");

    // request a GPU lease for the specified model
    let lease = state.scheduler.request_gpu(&payload.model, app_id).await;
    kprintln!(
        "-> GPU Lease Granted. Executing natively via {}...",
        state.driver.engine_name()
    );

    let current_fingerprint = Pager::get_history_fingerprint(app_id);
    let mut current_context = None;
    if manifest.resources.json_history {
        current_context = Some(Pager::page_in_history(app_id));
    }

    let (tx_driver, mut rx_driver) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (tx_stream, rx_stream) = tokio::sync::mpsc::unbounded_channel::<String>();

    let driver = Arc::clone(&state.driver);
    let scheduler_clone = Arc::clone(&state.scheduler);
    let model_name = lease.model.clone();
    let prompt = secured_gpu_prompt.clone();
    let app_id_str = app_id_owned;

    let is_stateful = manifest.resources.stateful_paging;
    let is_json_history = manifest.resources.json_history;

    tokio::spawn(async move {
        let driver_inner = Arc::clone(&driver);
        let m_name = model_name.clone();
        let a_id = app_id_str.clone();
        let p_text = prompt.clone();
        let ctx = current_context.clone();

        let gen_task = tokio::spawn(async move {
            if let Err(e) = driver_inner
                .generate_text(
                    &m_name,
                    &a_id,
                    is_stateful,
                    &p_text,
                    ctx,
                    tx_driver,
                    &current_fingerprint,
                )
                .await
            {
                kprintln!("-> [KERNEL ERROR] Inference execution failed: {}", e);
            }
        });

        let mut full_response = String::new();
        while let Some(word) = rx_driver.recv().await {
            full_response.push_str(&word);
            let _ = tx_stream.send(word);
        }

        let _ = gen_task.await;

        if is_json_history {
            let mut new_history = current_context.unwrap_or_default();
            new_history.push(ore_core::memory::ContextMessage {
                role: "user".to_string(),
                content: prompt.clone(),
            });
            new_history.push(ore_core::memory::ContextMessage {
                role: "assistant".to_string(),
                content: full_response.clone(),
            });

            Pager::page_out_history(&app_id_str, &new_history);

            let total_chars: usize = new_history.iter().map(|m| m.content.len()).sum();
            let estimated_tokens = (total_chars / 4) as u32;
            let token_limit = manifest.memory_limits.max_json_tokens;
            let token_cap_hit = estimated_tokens > token_limit;

            let mut kv_cap_hit = false;
            let mut current_kv_mb = 0;
            if is_stateful && manifest.memory_limits.max_kv_cache_mb > 0 {
                current_kv_mb = Pager::get_kv_cache_size_mb(&app_id_str, &model_name);
                kv_cap_hit = current_kv_mb > manifest.memory_limits.max_kv_cache_mb;
            }

            // The Compaction Engine
            if token_cap_hit || kv_cap_hit {
                if manifest.memory_limits.auto_summarize_on_cap {
                    kprintln!("-> Execution complete. Releasing GPU Lock for compaction.");
                    drop(lease);

                    if kv_cap_hit {
                        kprintln!(
                            "-> [KERNEL] User '{}' VRAM/SSD cap reached ({}MB > {}MB). Triggering Compaction...",
                            app_id_str,
                            current_kv_mb,
                            manifest.memory_limits.max_kv_cache_mb
                        );
                    } else {
                        kprintln!(
                            "-> [KERNEL] User '{}' context cap reached ({} > {} tokens). Triggering Compaction...",
                            app_id_str,
                            estimated_tokens,
                            token_limit
                        );
                    }

                    let text_to_summarize = new_history
                        .iter()
                        .map(|m| format!("{}: {}", m.role, m.content))
                        .collect::<Vec<String>>()
                        .join("\n");

                    let summary_prompt = format!(
                        "You are a system memory compressor. Extract all factual information, user preferences, names, numbers, and core context from the following raw conversation log. Output ONLY a dense bulleted list of facts. Do not converse:\n\n{}",
                        text_to_summarize
                    );

                    let comp_lease = scheduler_clone.request_gpu(&model_name, &app_id_str).await;
                    kprintln!("-> [COMPACTION] GPU Lease acquired for background summarization.");

                    let (tx_comp, mut rx_comp) = tokio::sync::mpsc::unbounded_channel::<String>();
                    let inner_driver = Arc::clone(&driver);
                    let inner_model = model_name.clone();
                    let inner_app_id = app_id_str.clone();

                    tokio::spawn(async move {
                        let _ = inner_driver
                            .generate_text(
                                &inner_model,
                                &inner_app_id,
                                false,
                                &summary_prompt,
                                None,
                                tx_comp,
                                "",
                            )
                            .await;
                    });

                    let mut summary = String::new();
                    while let Some(word) = rx_comp.recv().await {
                        summary.push_str(&word);
                    }

                    drop(comp_lease);

                    // OS SAFETY HEURISTIC 1: TRUNCATE BLOATED SUMMARIES
                    let safe_target = (token_limit as f32 * 0.75) as usize;
                    let max_summary_chars = safe_target * 4;

                    if summary.len() > max_summary_chars {
                        kprintln!(
                            "-> [COMPACTION] [WARN] AI generated a bloated summary. Truncating mechanically."
                        );
                        kprintln!(
                            "-> [COMPACTION] ACTION REQUIRED: Increase 'max_json_tokens' in the user manifest!"
                        );

                        // Safely slice the string at a valid UTF-8 character boundary
                        let safe_idx = summary
                            .char_indices()
                            .nth(max_summary_chars)
                            .map(|(i, _)| i)
                            .unwrap_or(summary.len());
                        summary = format!(
                            "{}... [TRUNCATED BY ORE KERNEL - INCREASE MEMORY LIMITS]",
                            &summary[..safe_idx]
                        );
                    }

                    let mut compacted_history = Vec::new();
                    compacted_history.push(ore_core::memory::ContextMessage {
                        role: "system".to_string(),
                        content: format!(
                            "You are a helpful AI assistant. Previous context summary:\n{}",
                            summary.trim()
                        ),
                    });

                    if new_history.len() >= 2 {
                        let len = new_history.len();
                        compacted_history.push(new_history[len - 2].clone());
                        compacted_history.push(new_history[len - 1].clone());
                    }

                    let new_estimated_tokens = (compacted_history
                        .iter()
                        .map(|m| m.content.len())
                        .sum::<usize>()
                        / 4) as u32;

                    if new_estimated_tokens > safe_target as u32 {
                        kprintln!(
                            "-> [COMPACTION] [WARN] AI failed to compress below limit. Forcing brutal FIFO pruning."
                        );
                        while compacted_history
                            .iter()
                            .map(|m| m.content.len())
                            .sum::<usize>()
                            / 4
                            > safe_target
                            && compacted_history.len() > 1
                        {
                            compacted_history.remove(1);
                        }
                    }

                    Pager::page_out_history(&app_id_str, &compacted_history);

                    if is_stateful {
                        Pager::delete_kv_cache(&app_id_str);
                        let _ = driver.invalidate_agent_cache(&app_id_str).await;
                        kprintln!(
                            "-> [COMPACTION] KV-Cache invalidated. VRAM Footprint reset to 0MB."
                        );
                    }
                } else {
                    kprintln!(
                        "-> [KERNEL] User '{}' memory cap reached. Pruning oldest messages (FIFO)...",
                        app_id_str
                    );
                    while new_history.iter().map(|m| m.content.len()).sum::<usize>() / 4
                        > token_limit as usize
                        && new_history.len() > 2
                    {
                        new_history.remove(0);
                    }
                    Pager::page_out_history(&app_id_str, &new_history);
                    if is_stateful {
                        Pager::delete_kv_cache(&app_id_str);
                        let _ = state.driver.invalidate_agent_cache(&app_id_str).await;
                    }
                    kprintln!("-> Execution complete. Releasing GPU Lock.");
                    drop(lease);
                }
            } else {
                kprintln!("-> Execution complete. Releasing GPU Lock.");
                drop(lease);
            }
        } else {
            kprintln!("-> Execution complete. Releasing GPU Lock.");
            drop(lease);
        }
    });

    let stream = UnboundedReceiverStream::new(rx_stream)
        .map(|chunk| Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(chunk)));

    Body::from_stream(stream).into_response()
}
