use ore_core::driver::InferenceDriver;
use ore_core::ipc::{MessageBus, RateLimiter, SemanticBus};
use ore_core::registry::AppRegistry;
use ore_core::scheduler::GpuScheduler;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize, Clone)]
pub struct OreConfig {
    pub system: SystemConfig,
    pub memory: MemoryConfig,
}

#[derive(Deserialize, Clone)]
pub struct SystemConfig {
    pub engine: String,
}

#[derive(Deserialize, Clone)]
pub struct MemoryConfig {
    pub cache_ttl_hours: u64, 
    pub pipe_ttl_hours: u64,
}

// kernel state shared across handlers
pub struct KernelState {
    pub driver: Arc<dyn InferenceDriver>,
    pub scheduler: Arc<GpuScheduler>,
    pub registry: AppRegistry,
    pub semantic_bus: Arc<SemanticBus>,
    pub message_bus: MessageBus,
    pub rate_limiter: RateLimiter,
    pub auth_token: String,
}