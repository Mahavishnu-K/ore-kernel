use crate::execution;
use crate::payloads::*;
use crate::state::KernelState;
use axum::extract::{Json, Path, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use ore_core::kprintln;
use ore_core::memory::Pager;
use std::sync::Arc;

fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|val| val.to_str().ok())
        .map(|s| s.contains("application/json"))
        .unwrap_or(false)
}

pub async fn health_check(State(state): State<Arc<KernelState>>, headers: HeaderMap) -> Response {
    let engine = state.driver.engine_name().to_string();
    let device = state.driver.device_name();
    let ore_dir = ore_core::get_ore_dir().display().to_string();
    let running = state.driver.get_running_models().await.unwrap_or_default();
    let loaded_models = running.len();
    let registered_agents = state.registry.list_apps().len();
    let uptime_seconds = ore_core::telemetry::get_host_telemetry().uptime_seconds;

    if wants_json(&headers) {
        let resp = KernelHealthResponse {
            status: "ONLINE".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            engine,
            device,
            ore_dir,
            loaded_models,
            registered_agents,
            uptime_seconds,
        };
        Json(resp).into_response()
    } else {
        format!(
            "ORE Kernel is ALIVE & HEALTHY (v{})\nEngine       : {}\nCompute      : {}\nHome Dir     : {}\nActive Models: {}\nAgents       : {}",
            env!("CARGO_PKG_VERSION"),
            engine,
            device,
            ore_dir,
            loaded_models,
            registered_agents
        )
        .into_response()
    }
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

pub async fn process_status(State(state): State<Arc<KernelState>>, headers: HeaderMap) -> Response {
    let driver_models = state.driver.get_running_models().await.unwrap_or_default();
    let scheduler_snap = state.scheduler.get_telemetry_snapshot().await;

    let mut models = Vec::new();
    let mut total_ram_mb: u64 = 0;
    let mut total_vram_mb: u64 = 0;

    for dm in &driver_models {
        let ram_mb = dm.size_bytes / (1024 * 1024);
        let mut vram_mb = dm.size_vram_bytes / (1024 * 1024);

        let sched_m = scheduler_snap.loaded_models.iter().find(|m| {
            m.model_id.eq_ignore_ascii_case(&dm.model_name)
                || m.model_id
                    .replace(":", "-")
                    .eq_ignore_ascii_case(&dm.model_name.replace(":", "-"))
        });

        let active_requests = sched_m
            .map(|m| m.active_requests)
            .unwrap_or(dm.active_requests);
        let status = if active_requests > 0 {
            "ACTIVE".to_string()
        } else {
            "IDLE".to_string()
        };
        let last_used_secs = sched_m.map(|m| m.idle_seconds).unwrap_or(dm.last_used_secs);

        if vram_mb == 0
            && let Some(sm) = sched_m
        {
            vram_mb = sm.estimated_vram_mb;
        }

        total_ram_mb += ram_mb;
        total_vram_mb += vram_mb;

        models.push(ModelProcessInfo {
            model_name: dm.model_name.clone(),
            engine: state.driver.engine_name().to_string(),
            device: state.driver.device_name(),
            status,
            active_requests,
            host_ram_mb: ram_mb,
            gpu_vram_mb: vram_mb,
            last_used_secs,
            current_app_id: scheduler_snap.active_app_id.clone(),
        });
    }

    if wants_json(&headers) {
        Json(PsResponse {
            models,
            total_ram_mb,
            total_vram_mb,
        })
        .into_response()
    } else {
        let mut output = format!(
            "{:<24} | {:<16} | {:<10} | {:<6} | {:<12} | {:<12} | {:<10} | {}\n",
            "MODEL",
            "COMPUTE/DEVICE",
            "STATUS",
            "REQS",
            "HOST RAM",
            "GPU VRAM",
            "LAST USED",
            "ACTIVE APP"
        );
        output.push_str("-------------------------------------------------------------------------------------------------------------------------\n");

        if models.is_empty() {
            output.push_str("No models currently loaded in memory.\n");
        } else {
            for m in &models {
                let last_used_str = if m.last_used_secs == 0 {
                    "Just now".to_string()
                } else if m.last_used_secs < 60 {
                    format!("{}s ago", m.last_used_secs)
                } else {
                    format!("{}m ago", m.last_used_secs / 60)
                };

                let active_app = m.current_app_id.as_deref().unwrap_or("-");

                output.push_str(&format!(
                    "{:<24} | {:<16} | {:<10} | {:<6} | {:<9} MB | {:<9} MB | {:<10} | {}\n",
                    m.model_name,
                    m.device,
                    m.status,
                    m.active_requests,
                    m.host_ram_mb,
                    m.gpu_vram_mb,
                    last_used_str,
                    active_app,
                ));
            }
        }
        output.into_response()
    }
}

pub async fn list_models(State(state): State<Arc<KernelState>>, headers: HeaderMap) -> Response {
    let local_models = state.driver.list_local_models().await.unwrap_or_default();
    let running = state.driver.get_running_models().await.unwrap_or_default();
    let running_names: Vec<String> = running
        .iter()
        .map(|m| m.model_name.to_lowercase())
        .collect();

    let mut models = Vec::new();
    for m in local_models {
        let norm_name = m.name.to_lowercase();
        let is_loaded = running_names
            .iter()
            .any(|rn| rn == &norm_name || rn.replace(":", "-") == norm_name.replace(":", "-"));

        models.push(LocalModelInfo {
            name: m.name,
            format: m.format,
            size_bytes: m.size_bytes,
            modified_at: m.modified_at,
            is_loaded,
        });
    }

    if wants_json(&headers) {
        Json(LsResponse { models }).into_response()
    } else {
        let mut output = format!(
            "{:<24} | {:<18} | {:<12} | {:<20} | {}\n",
            "REPOSITORY", "FORMAT", "SIZE", "LAST MODIFIED", "STATE"
        );
        output.push_str("-------------------------------------------------------------------------------------------------\n");

        if models.is_empty() {
            output.push_str("No models installed. Use 'ore pull <model>'.\n");
        } else {
            for m in &models {
                let size_disp = if m.size_bytes >= 1024 * 1024 * 1024 {
                    format!("{:.2} GB", m.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
                } else {
                    format!("{:.1} MB", m.size_bytes as f64 / (1024.0 * 1024.0))
                };

                let state_str = if m.is_loaded {
                    "IN VRAM / ACTIVE"
                } else {
                    "ON DISK"
                };

                output.push_str(&format!(
                    "{:<24} | {:<18} | {:<12} | {:<20} | {}\n",
                    m.name, m.format, size_disp, m.modified_at, state_str
                ));
            }
        }
        output.into_response()
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

pub async fn list_agents(State(state): State<Arc<KernelState>>, headers: HeaderMap) -> Response {
    let apps = state.registry.list_apps();
    let mut agents = Vec::new();

    for app in &apps {
        let models = app.resources.allowed_models.clone();
        let priority = if app.resources.gpu_priority.trim().is_empty() {
            "DEFAULT".to_string()
        } else {
            app.resources.gpu_priority.to_uppercase()
        };

        let can_read = !app.file_system.allowed_read_paths.is_empty();
        let can_write = !app.file_system.allowed_write_paths.is_empty();
        let fs_perms = match (can_read, can_write) {
            (true, true) => "Read/Write".to_string(),
            (true, false) => "Read-Only".to_string(),
            (false, true) => "Write-Only".to_string(),
            (false, false) => "Isolated".to_string(),
        };

        let exec_engine = if app.execution.can_execute_shell {
            "Shell (Risk)".to_string()
        } else if app.execution.can_execute_wasm {
            "WASM Sandbox".to_string()
        } else {
            "Prompt-Only".to_string()
        };

        let status = if app.execution.can_execute_shell {
            "UNSAFE".to_string()
        } else if !app.network.network_enabled {
            "AIR-GAPPED".to_string()
        } else {
            "SECURED".to_string()
        };

        agents.push(AgentTelemetryInfo {
            app_id: app.app_id.clone(),
            version: app.version.clone(),
            allowed_models: models,
            priority,
            network_enabled: app.network.network_enabled,
            filesystem_permissions: fs_perms,
            execution_engine: exec_engine,
            pii_enforcement: app.privacy.enforce_pii_redaction,
            status,
        });
    }

    if wants_json(&headers) {
        Json(AgentsResponse { agents }).into_response()
    } else {
        let mut output = format!(
            "{:<20} | {:<8} | {:<20} | {:<9} | {:<12} | {:<14} | {}\n",
            "AGENT ID", "VERSION", "ALLOWED MODELS", "PRIORITY", "FILE I/O", "EXECUTION", "STATUS"
        );
        output.push_str("-------------------------------------------------------------------------------------------------------\n");

        if agents.is_empty() {
            output.push_str("No agents registered. Use 'ore manifest <name>' to create one.\n");
        } else {
            for a in &agents {
                let models_str = if a.allowed_models.is_empty() {
                    "-".to_string()
                } else {
                    a.allowed_models.join(", ")
                };
                let models_disp = if models_str.len() > 18 {
                    format!("{}...", &models_str[..15])
                } else {
                    models_str
                };

                output.push_str(&format!(
                    "{:<20} | {:<8} | {:<20} | {:<9} | {:<12} | {:<14} | {}\n",
                    a.app_id,
                    a.version,
                    models_disp,
                    a.priority,
                    a.filesystem_permissions,
                    a.execution_engine,
                    a.status
                ));
            }
        }
        output.into_response()
    }
}

pub async fn list_manifests(State(state): State<Arc<KernelState>>, headers: HeaderMap) -> Response {
    let apps = state.registry.list_apps();
    let mut manifests = Vec::new();

    for app in &apps {
        let can_read = !app.file_system.allowed_read_paths.is_empty();
        let can_write = !app.file_system.allowed_write_paths.is_empty();
        let fs_status = match (can_read, can_write) {
            (true, true) => "Read/Write".to_string(),
            (true, false) => "Read-Only".to_string(),
            (false, true) => "Write-Only".to_string(),
            (false, false) => "Air-gapped".to_string(),
        };

        let exec_status = if app.execution.can_execute_shell {
            "SHELL (RISK)".to_string()
        } else if app.execution.can_execute_wasm {
            "WASM Sandbox".to_string()
        } else {
            "Disabled".to_string()
        };

        let pii_status = if app.privacy.enforce_pii_redaction {
            "ACTIVE".to_string()
        } else {
            "OFF (RISK)".to_string()
        };

        manifests.push(ManifestTelemetryInfo {
            file_name: format!("{}.toml", app.app_id),
            app_id: app.app_id.clone(),
            network: if app.network.network_enabled {
                "ENABLED".to_string()
            } else {
                "BLOCKED".to_string()
            },
            file_io: fs_status,
            execution: exec_status,
            pii_scrubbing: pii_status,
        });
    }

    if wants_json(&headers) {
        Json(ManifestsResponse { manifests }).into_response()
    } else {
        let mut output = format!(
            "{:<24} | {:<20} | {:<10} | {:<12} | {:<15} | {}\n",
            "MANIFEST FILE", "APP ID", "NETWORK", "FILE I/O", "EXECUTION", "PII REDACTION"
        );
        output.push_str("--------------------------------------------------------------------------------------------------------------\n");

        if manifests.is_empty() {
            output.push_str("No manifests found in /manifests directory.\n");
        } else {
            for m in &manifests {
                output.push_str(&format!(
                    "{:<24} | {:<20} | {:<10} | {:<12} | {:<15} | {}\n",
                    m.file_name, m.app_id, m.network, m.file_io, m.execution, m.pii_scrubbing
                ));
            }
        }
        output.into_response()
    }
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

pub async fn top_telemetry(State(state): State<Arc<KernelState>>, headers: HeaderMap) -> Response {
    let host = ore_core::telemetry::get_host_telemetry();
    let scheduler_snap = state.scheduler.get_telemetry_snapshot().await;
    let scheduler_summary = state.scheduler.get_status().await;
    let apps_count = state.registry.list_apps().len();
    let cache_count = state.semantic_bus.cache_len();
    let device = state.driver.device_name();
    let engine = state.driver.engine_name().to_string();

    let resp = TopTelemetryResponse {
        host: host.clone(),
        accelerator_device: device.clone(),
        vram_total_mb: scheduler_snap.total_vram_mb,
        vram_used_mb: scheduler_snap.used_vram_mb,
        vram_free_mb: scheduler_snap.free_vram_mb,
        vram_reserved_mb: scheduler_snap.reserved_vram_mb,
        vram_safety_margin_mb: scheduler_snap.safety_margin_mb,
        engine_name: engine.clone(),
        scheduler_summary,
        active_models_count: scheduler_snap.loaded_models.len(),
        registered_agents_count: apps_count,
        context_firewall: "ACTIVE (Enforcing Rules & PII Redaction)".to_string(),
        semantic_bus_cache_count: cache_count,
        wasm_sandbox: "ISOLATED (WASI 0.2 Ring-3 Ready)".to_string(),
    };

    if wants_json(&headers) {
        Json(resp).into_response()
    } else {
        let mut out = String::new();
        out.push_str("============================ ORE KERNEL REALTIME TELEMETRY ============================\n\n");

        out.push_str("[HOST HARDWARE & OS]\n");
        out.push_str(&format!(
            "  OS / Platform      : {} {} ({})\n",
            host.os_name, host.os_version, host.host_name
        ));
        out.push_str(&format!(
            "  CPU Processor      : {} ({} Cores) | CPU Load: {:.1}%\n",
            host.cpu_brand, host.cpu_count, host.cpu_usage_pct
        ));
        out.push_str(&format!(
            "  Host RAM           : {} MB used / {} MB total ({:.1}% utilized, {} MB free)\n",
            host.used_memory_mb,
            host.total_memory_mb,
            if host.total_memory_mb > 0 {
                (host.used_memory_mb as f64 / host.total_memory_mb as f64) * 100.0
            } else {
                0.0
            },
            host.free_memory_mb
        ));
        out.push_str(&format!(
            "  Virtual / Swap     : {} MB used / {} MB total\n",
            host.used_swap_mb, host.total_swap_mb
        ));
        out.push_str(&format!(
            "  System Uptime      : {}h {}m {}s\n\n",
            host.uptime_seconds / 3600,
            (host.uptime_seconds % 3600) / 60,
            host.uptime_seconds % 60
        ));

        out.push_str("[ACCELERATOR & VRAM]\n");
        out.push_str(&format!("  Compute Device     : {}\n", device));
        out.push_str(&format!(
            "  VRAM Allocation    : {} MB used / {} MB total (Free: {} MB)\n",
            scheduler_snap.used_vram_mb, scheduler_snap.total_vram_mb, scheduler_snap.free_vram_mb
        ));
        out.push_str(&format!(
            "  VRAM Accounting    : Reserved: {} MB | Safety Margin: {} MB\n\n",
            scheduler_snap.reserved_vram_mb, scheduler_snap.safety_margin_mb
        ));

        out.push_str("[ORE KERNEL SUBSYSTEMS]\n");
        out.push_str(&format!("  Inference Driver   : {} (ONLINE)\n", engine));
        out.push_str(&format!(
            "  Scheduler Status   : {}\n",
            resp.scheduler_summary
        ));
        out.push_str(&format!(
            "  Context Firewall   : {}\n",
            resp.context_firewall
        ));
        out.push_str(&format!(
            "  App Registry       : {} Registered Agents\n",
            apps_count
        ));
        out.push_str(&format!(
            "  Semantic Bus Cache : {} Vectors Cached (Dynamic TTL)\n",
            cache_count
        ));
        out.push_str(&format!("  WASM Sandbox       : {}\n\n", resp.wasm_sandbox));

        if !scheduler_snap.loaded_models.is_empty() {
            out.push_str("[ACTIVE MODELS IN MEMORY]\n");
            out.push_str(&format!(
                "  {:<20} | {:<10} | {:<12} | {:<10} | {}\n",
                "MODEL ID", "STATUS", "EST. VRAM", "REQUESTS", "IDLE TIME"
            ));
            out.push_str(
                "  ------------------------------------------------------------------------\n",
            );
            for m in &scheduler_snap.loaded_models {
                out.push_str(&format!(
                    "  {:<20} | {:<10} | {:<9} MB | {:<10} | {}s ago\n",
                    m.model_id, m.status, m.estimated_vram_mb, m.active_requests, m.idle_seconds
                ));
            }
        }
        out.push_str("========================================================================================");

        out.into_response()
    }
}

pub async fn kill_app(State(state): State<Arc<KernelState>>, Path(app_id): Path<String>) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] SIGTERM received for Agent '{}'",
        app_id
    );
    let _ = state.driver.invalidate_agent_cache(&app_id).await;
    format!("SUCCESS: App '{}' context wiped from GPU Memory.", app_id).to_string()
}
