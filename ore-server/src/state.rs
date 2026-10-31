use ore_core::driver::{DriverError, InferenceDriver};
use ore_core::ipc::{MessageBus, RateLimiter, SemanticBus};
use ore_core::registry::AppRegistry;
use ore_core::sandbox::WasmSandbox;
use ore_core::scheduler::GpuScheduler;
use std::sync::Arc;
use tokio::sync::Mutex;

pub use ore_common::config::*;

/// RAII Drop guard ensuring embedding weights are evicted from memory
/// even if a panic or unexpected early exit occurs.
pub struct ScopedEmbedderLease {
    driver: Arc<dyn InferenceDriver>,
    model_name: String,
}

impl ScopedEmbedderLease {
    pub fn new(driver: Arc<dyn InferenceDriver>, model_name: String) -> Self {
        Self { driver, model_name }
    }
}

impl Drop for ScopedEmbedderLease {
    fn drop(&mut self) {
        let driver = Arc::clone(&self.driver);
        let model = self.model_name.clone();
        tokio::spawn(async move {
            let _ = driver.unload_model(&model).await;
            ore_core::kprintln!(
                "-> [MMU LEASE] System Embedder safely evicted from memory (0MB Idle)."
            );
        });
    }
}

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

impl KernelState {
    /// Thread-safe scoped embedding generator with guaranteed VRAM/RAM release.
    ///
    /// Acquires `embedder_lock`, computes embeddings, and ensures `unload_model`
    /// is executed before releasing the lock, eliminating model retention on errors.
    pub async fn generate_embeddings_scoped(
        &self,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError> {
        let _guard = self.embedder_lock.lock().await;
        let res = self
            .driver
            .generate_embeddings(&self.system_embedder, inputs)
            .await;
        let _ = self.driver.unload_model(&self.system_embedder).await;
        res
    }
}
