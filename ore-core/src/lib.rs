use ore_common::{InferenceRequest, InferenceResponse}; // Fixed: Added semicolon
use std::sync::Arc;
use tokio::sync::Mutex;
use thiserror::Error; // Fixed: :: instead of :

#[derive(Error, Debug)]
pub enum OreError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Engine busy")]
    EngineBusy,
}

pub struct OreEngine {
    active_model: Arc<Mutex<Option<String>>>,
}

impl OreEngine {
    pub fn new() -> Self {
        Self {
            active_model: Arc::new(Mutex::new(None)),
        }
    }

    // Fixed: Changed &Self to &self (lowercase s is for 'this instance')
    pub async fn infer(&self, req: InferenceRequest) -> Result<InferenceResponse, OreError> {
        
        // Fixed: Changed Self.active_model to self.active_model
        let mut _guard = self.active_model.lock().await;

        println!("Core: Processing request for model {:?}", req.model_id);
        
        // Simulation delay
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        Ok(InferenceResponse {
            content: format!("Processed: {}", req.prompt),
            token_usage: 10,
        })
    }
}