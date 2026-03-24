use crate::swap::ContextMessage;
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

#[derive(Debug, Clone)]
pub struct LocalModel {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

// OS DATA STRUCTURES
// No matter what engine is running, ORE translates their data into this.
#[derive(Debug, Clone)]
pub struct VramProcess {
    pub model_name: String,
    pub size_bytes: u64,
    pub size_vram_bytes: u64,
}

// HARDWARE ABSTRACTION LAYER (HAL)
// Any backend (Ollama, LM Studio, vLLM) MUST implement these functions.
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    fn engine_name(&self) -> &'static str;

    async fn is_online(&self) -> bool;

    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError>;

    async fn unload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn preload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError>;

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError>;

    async fn generate_text(
        &self,
        model: &str,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
    ) -> Result<(), DriverError>;

    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError>;
}
