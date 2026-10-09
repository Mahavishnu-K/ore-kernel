use crate::execution;
use crate::payloads::ExecuteRequest;
use crate::state::KernelState;
use axum::extract::{Json, Path, State};
use ore_core::kprintln;
use ore_core::memory::Pager;
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
    match execution::execute_request(state, payload).await {
        Ok(output) => output,
        Err(err) => format!("KERNEL ERROR: {}", err),
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
    let lease = match state.scheduler.request_gpu(target_model, &app_id).await {
        Ok(l) => l,
        Err(e) => return format!("ORE KERNEL ALERT: GPU unavailable - {}", e),
    };

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
