pub mod handlers;
pub mod middleware;
pub mod payloads;
pub mod state;

use axum::{
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};
use std::fs;
use std::sync::Arc;
use tokio::net::TcpListener;
use uuid::Uuid;

use ore_core::driver::InferenceDriver;
use ore_core::external::ollama::OllamaDriver;
use ore_core::ipc::{MessageBus, RateLimiter, SemanticBus};
use ore_core::native::NativeDriver;
use ore_core::registry::AppRegistry;
use ore_core::scheduler::GpuScheduler;

use crate::middleware::auth_middleware;
use crate::state::{KernelState, OreConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ORE SYSTEM KERNEL BOOTING ===");

    let session_token = Uuid::new_v4().to_string();
    fs::write("ore-kernel.token", &session_token).expect("Failed to write security token.");
    println!("-> [SECURITY] Master Token generated and secured to disk.");

    println!("-> Sweeping /manifests for installed Apps...");
    let app_registry =
        AppRegistry::boot_load("../manifests").expect("FATAL: Failed to initialize App Registry");

    let config_str =
        fs::read_to_string("../ore.toml").expect("FATAL: ore.toml missing. Run 'ore init'");
    let config: OreConfig = toml::from_str(&config_str).unwrap();

    let driver: Arc<dyn InferenceDriver> = if config.system.engine == "native" {
        println!("-> [BOOT] Engaging Native Candle Engine...");
        Arc::new(NativeDriver::new())
    } else {
        println!("-> [BOOT] Engaging Ollama API Driver...");
        Arc::new(OllamaDriver::new("http://127.0.0.1:11434"))
    };

    let semantic_bus =
        SemanticBus::new(config.memory.cache_ttl_hours, config.memory.pipe_ttl_hours);

    let shared_semantic_bus = Arc::new(semantic_bus);

    // configuration
    let shared_state = Arc::new(KernelState {
        driver,
        scheduler: Arc::new(GpuScheduler::new()),
        registry: app_registry,
        semantic_bus: Arc::clone(&shared_semantic_bus),
        message_bus: MessageBus::new(),
        rate_limiter: RateLimiter::new(),
        auth_token: session_token,
    });

    let app = Router::new()
        .route("/health", get(handlers::system::health_check))
        .route("/ps", get(handlers::system::process_status))
        .route("/ls", get(handlers::system::list_models))
        .route("/agents", get(handlers::system::list_agents))
        .route("/manifests", get(handlers::system::list_manifests))
        .route("/expel/:model", get(handlers::system::expel_model))
        .route("/pull/:model", get(handlers::system::pull_model))
        .route("/load/:model", get(handlers::system::load_model))
        .route("/clear/:app_id", get(handlers::system::clear_memory))
        .route("/ask/:prompt", get(handlers::inference::ask_ai))
        .route("/run", post(handlers::inference::run_process))
        .route("/ipc/share", post(handlers::ipc::sys_share_context))
        .route("/ipc/search", post(handlers::ipc::sys_search_context))
        .route("/ipc/send", post(handlers::ipc::ipc_send))
        .route("/ipc/listen/:app_id", get(handlers::ipc::ipc_listen))
        .layer(axum_middleware::from_fn_with_state(
            shared_state.clone(),
            auth_middleware,
        ))
        .with_state(shared_state.clone());

    let gc_bus = shared_state.semantic_bus.clone();
    tokio::spawn(async move {
        loop {
            // Wake up every 1 hour (3600 seconds)
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            println!("-> [SYSTEM] Running routine memory Garbage Collection...");
            gc_bus.run_garbage_collection();
        }
    });

    let addr = "127.0.0.1:3000";
    println!("=== ORE KERNEL IS ONLINE ===");
    println!("Listening on http://{}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    let _ = fs::remove_file("ore-kernel.token");
    Ok(())
}
