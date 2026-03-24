use crate::state::KernelState;
use axum::extract::{Path, State};
use ore_core::swap::Pager;
use std::sync::Arc;

pub async fn health_check(State(state): State<Arc<KernelState>>) -> String {
    format!(
        "ORE Kernel is ALIVE. Powered by: {}",
        state.driver.engine_name()
    )
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
                output.push_str("No models installed. Use 'ore install <model>'.\n");
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

pub async fn clear_memory(Path(app_id): Path<String>) -> String {
    println!(
        "-> [KERNEL COMMAND] Wiping SSD Memory for Agent '{}'",
        app_id
    );
    Pager::clear_page(&app_id);
    format!(
        "SUCCESS: Memory for Agent '{}' has been wiped clean.",
        app_id
    )
}
