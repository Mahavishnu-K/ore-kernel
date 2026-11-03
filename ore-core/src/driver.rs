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

/// What an embedding is for. Asymmetric retrieval embedders (NomicBert) are trained
/// with a different task prefix for stored passages and for search queries; vectors
/// only line up when each side uses its own prefix. Symmetric embedders (BERT/MiniLM)
/// and external drivers ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbedTask {
    /// Text being stored for later retrieval (chunks, tool findings, HyDE excerpts).
    Document,
    /// A search query compared against stored documents.
    Query,
}

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
        task: EmbedTask,
    ) -> Result<Vec<Vec<f32>>, DriverError>;

    async fn flush_idle_memory(&self, idle_timeout_mins: u64) -> Result<(), DriverError>;

    async fn invalidate_agent_cache(&self, app_id: &str) -> Result<(), DriverError>;

    /// Returns the exact token count for the given text using the engine's tokenizer,
    /// or `None` if the driver does not have direct tokenizer access.
    async fn count_tokens(&self, _model: &str, _text: &str) -> Option<u32> {
        None
    }
}
