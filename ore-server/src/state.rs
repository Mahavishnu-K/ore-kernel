use ore_core::driver::InferenceDriver;
use ore_core::ipc::{MessageBus, RateLimiter, SemanticBus};
use ore_core::registry::AppRegistry;
use ore_core::sandbox::WasmSandbox;
use ore_core::scheduler::GpuScheduler;
use std::sync::Arc;
use tokio::sync::Mutex;

pub use ore_common::config::*;

// kernel state shared across handlers
pub struct KernelState {
    pub driver: Arc<dyn InferenceDriver>,
    pub scheduler: Arc<GpuScheduler>,
    pub embedder_lock: Arc<Mutex<()>>,
    pub registry: AppRegistry,
    pub semantic_bus: Arc<SemanticBus>,
    pub message_bus: MessageBus,
    pub rate_limiter: RateLimiter,
    pub auth_token: String,
    pub system_embedder: String,
    pub sandbox: Arc<WasmSandbox>,
}
