use crate::memory::ContextMessage;
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("Driver Offline or Unreachable: {0}")]
    ConnectionFailed(String),
    #[error("API Error: {0}")]
    ApiError(String),
    #[error("Execution Failed: {0}")]
    ExecutionFailed(String),
}

pub use ore_common::types::{LocalModel, VramProcess};

// HARDWARE ABSTRACTION LAYER (HAL)
// Any backend (Ollama, LM Studio, vLLM) MUST implement these functions.
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    fn engine_name(&self) -> &'static str;

    fn device_name(&self) -> String {
        "Default Device".to_string()
    }

    async fn is_online(&self) -> bool;

    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError>;

    async fn unload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn preload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError>;

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError>;

    #[allow(clippy::too_many_arguments)]
    async fn generate_text(
        &self,
        model: &str,
        app_id: &str,
        stateful_paging: bool,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
        current_fingerprint: &str,
    ) -> Result<(), DriverError>;

    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError>;

    async fn flush_idle_memory(&self, idle_timeout_mins: u64) -> Result<(), DriverError>;

    async fn invalidate_agent_cache(&self, app_id: &str) -> Result<(), DriverError>;
}
