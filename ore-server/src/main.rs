use axum::{
    extract::{Path, State},
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use serde::{Deserialize, Serialize};

// -------------------------------------------------------------
// THE KERNEL STATE
// -------------------------------------------------------------
struct KernelState {
    ollama_url: String,
    model_name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ORE SYSTEM KERNEL BOOTING (OLLAMA DRIVER) ===");

    // Configuration
    let shared_state = Arc::new(KernelState { 
        ollama_url: "http://127.0.0.1:11434".to_string(), // Default Ollama port
        model_name: "llama3.2:1b".to_string(),               // The model we will use
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ask/:prompt", get(ask_ai))
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

// -------------------------------------------------------------
// OLLAMA REQUEST/RESPONSE STRUCTURES
// -------------------------------------------------------------
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool, 
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
    done: bool,
}

// -------------------------------------------------------------
// INFERENCE ENGINE (The Proxy & Firewall)
// -------------------------------------------------------------
async fn ask_ai(
    State(state): State<Arc<KernelState>>,
    Path(prompt): Path<String>,
) -> String {
    let clean_prompt = prompt.replace("_", " ");
    
    println!("\n========================================");
    println!("-> Incoming App Request: {}", clean_prompt);
    
    // =========================================================
    // 1. ORE SECURITY LAYER (The Firewall)
    // This is the magic of ORE. We intercept malicious prompts.
    // =========================================================
    let lower_prompt = clean_prompt.to_lowercase();
    if lower_prompt.contains("password") || lower_prompt.contains("delete system") {
        println!("-> [BLOCKED] Security Threat Detected. Dropping request.");
        return "ORE KERNEL ALERT: Request blocked by Firewall. Access Denied.".to_string();
    }

    println!("-> Security Check Passed. Routing to Ollama Driver...");

    // 2. Prepare the Request for Ollama
    let client = reqwest::Client::new();
    let request_body = OllamaRequest {
        model: state.model_name.clone(),
        prompt: clean_prompt,
        stream: false,
    };

    // 3. Send to Ollama
    let res = client
        .post(format!("{}/api/generate", state.ollama_url))
        .json(&request_body)
        .send()
        .await;

    // 4. Return the Answer
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