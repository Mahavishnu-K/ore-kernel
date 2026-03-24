use crate::payloads::RunRequest;
use crate::state::KernelState;
use axum::body::Body;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ore_core::firewall::ContextFirewall;
use ore_core::swap::Pager;
use std::sync::Arc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

// inference engine (The Proxy & Firewall)
pub async fn ask_ai(State(state): State<Arc<KernelState>>, Path(prompt): Path<String>) -> String {
    let clean_prompt = prompt.replace("_", " ");

    println!("\n-> Incoming App Request: {}", clean_prompt);

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

    let mut current_context = None;
    if manifest.resources.stateful_paging {
        current_context = Some(Pager::page_in_history(app_id));
    }

    println!("-> Waiting for GPU Scheduler...");

    // If the agent lists allowed_models, pick the first one. Default to "llama3.2:1b"
    let target_model = manifest
        .resources
        .allowed_models
        .first()
        .map(|s| s.as_str())
        .unwrap_or("llama3.2:1b");

    // the GPU scheduler
    let lease = state.scheduler.request_gpu(target_model).await;
    println!(
        "-> GPU Lease Granted for '{}'. Routing to Driver...",
        lease.model
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let driver = Arc::clone(&state.driver);
    let model_name = lease.model.clone();
    let prompt_clone = secured_prompt.clone();
    let context_clone = current_context.clone();

    tokio::spawn(async move {
        let _ = driver
            .generate_text(&model_name, &prompt_clone, context_clone, tx)
            .await;

        println!("-> Agent Execution complete. Releasing GPU Lock.");
        drop(lease);
    });

    let mut full_response = String::new();
    while let Some(word) = rx.recv().await {
        full_response.push_str(&word);
    }

    if manifest.resources.stateful_paging {
        let mut new_history = current_context.unwrap_or_default();
        new_history.push(ore_core::swap::ContextMessage {
            role: "user".to_string(),
            content: secured_prompt,
        });
        new_history.push(ore_core::swap::ContextMessage {
            role: "assistant".to_string(),
            content: full_response.clone(),
        });
        Pager::page_out_history(app_id, &new_history);
    }

    full_response
}

pub async fn run_process(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<RunRequest>,
) -> Response {
    println!(
        "-> [EXEC] Model: {} | Prompt: {}",
        payload.model, payload.prompt
    );

    let app_id = "terminal_user";
    let manifest = match state.registry.get_app(app_id) {
        Some(m) => m,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                format!("ORE KERNEL ALERT: Unregistered User '{}'.", app_id),
            )
                .into_response()
        }
    };

    // future update: calculate tokens based on prompt length or use a more dynamic approach
    let limit = manifest.resources.max_tokens_per_minute;
    if !state.rate_limiter.check_and_add(app_id, limit, 1000) {
        println!("-> [BLOCKED] Agent '{}' exceeded GPU rate limit.", app_id);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            format!("ORE KERNEL ALERT: Rate Limit Exceeded ({} t/min).", limit),
        )
            .into_response();
    }

    let secured_prompt = match ContextFirewall::secure_request(manifest, &payload.prompt) {
        Ok(safe_text) => {
            println!("-> Security Check Passed.");
            safe_text
        }
        Err(e) => {
            println!("-> [BLOCKED] {}", e);
            return (StatusCode::FORBIDDEN, format!("ORE KERNEL ALERT: {}", e)).into_response();
        }
    };

    println!("-> Waiting for GPU Scheduler...");

    // request a GPU lease for the specified model
    let lease = state.scheduler.request_gpu(&payload.model).await;
    println!(
        "-> GPU Lease Granted. Executing natively via {}...",
        state.driver.engine_name()
    );

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let driver = Arc::clone(&state.driver);
    let model_name = lease.model.clone();
    let prompt = secured_prompt.clone();

    tokio::spawn(async move {
        let _ = driver.generate_text(&model_name, &prompt, None, tx).await;

        println!("-> Execution complete. Releasing GPU Lock.");

        drop(lease);
    });

    let stream = UnboundedReceiverStream::new(rx)
        .map(|chunk| Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(chunk)));

    Body::from_stream(stream).into_response()
}
