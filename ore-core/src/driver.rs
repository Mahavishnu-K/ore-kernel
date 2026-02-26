use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("Driver Offline or Unreachable: {0}")]
    ConnectionFailed(String),
    #[error("API Error: {0}")]
    ApiError(String),
}

#[derive(Debug, Clone)]
pub struct LocalModel {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

// =====================================================================
// 1. THE OS DATA STRUCTURES
// No matter what engine is running, ORE translates their data into this.
// =====================================================================
#[derive(Debug, Clone)]
pub struct VramProcess {
    pub model_name: String,
    pub size_bytes: u64,
    pub size_vram_bytes: u64,
}

// =====================================================================
// 2. THE HARDWARE ABSTRACTION LAYER (HAL)
// Any backend (Ollama, LM Studio, vLLM) MUST implement these functions.
// =====================================================================
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    async fn is_online(&self) -> bool;
    
    // NEW: Ask the driver exactly what is loaded in the GPU right now
    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError>;
    
    // The actual math execution
    async fn generate(&self, prompt: &str, model: &str) -> Result<String, DriverError>;

    async fn unload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn preload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError>;

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError>;
}

// =====================================================================
// 3. THE OLLAMA IMPLEMENTATION
// =====================================================================
pub struct OllamaDriver {
    pub base_url: String,
    client: Client,
}

impl OllamaDriver {
    pub fn new(url: &str) -> Self {
        Self {
            base_url: url.to_string(),
            client: Client::new(),
        }
    }
}

// Ollama's specific JSON response format for `/api/ps`
#[derive(Deserialize)]
struct OllamaPsResponse {
    models: Vec<OllamaModelProcess>,
}

#[derive(Deserialize)]
struct OllamaModelProcess {
    name: String,
    size: u64,
    size_vram: u64,
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagModel>,
}

#[derive(Deserialize)]
struct OllamaTagModel {
    name: String,
    size: u64,
    modified_at: String,
}

#[async_trait]
impl InferenceDriver for OllamaDriver {
    async fn is_online(&self) -> bool {
        self.client.get(&self.base_url).send().await.is_ok()
    }

    // TAKING CONTROL: This scans Ollama's RAM/VRAM
    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError> {
        let url = format!("{}/api/ps", self.base_url);
        let res = self.client.get(&url).send().await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(DriverError::ApiError(format!("Ollama returned {}", res.status())));
        }

        let data: OllamaPsResponse = res.json().await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        // Translate Ollama's JSON into ORE's standard Process list
        let processes = data.models.into_iter().map(|m| VramProcess {
            model_name: m.name,
            size_bytes: m.size,
            size_vram_bytes: m.size_vram,
        }).collect();

        Ok(processes)
    }

    async fn generate(&self, prompt: &str, model: &str) -> Result<String, DriverError> {
        let url = format!("{}/api/generate", self.base_url);
        let payload = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false
        });

        let res = self.client.post(&url).json(&payload).send().await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        let data: OllamaGenerateResponse = res.json().await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        Ok(data.response)
    }

    async fn unload_model(&self, model_name: &str) -> Result<(), DriverError> {
        let url = format!("{}/api/generate", self.base_url);
        
        // Setting keep_alive to 0 tells the driver to drop it from RAM
        let payload = serde_json::json!({
            "model": model_name,
            "keep_alive": 0 
        });

        let res = self.client.post(&url).json(&payload).send().await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if res.status().is_success() {
            Ok(())
        } else {
            Err(DriverError::ApiError(format!("Failed to unload: {}", res.status())))
        }
    }

    async fn preload_model(&self, model_name: &str) -> Result<(), DriverError> {
        let url = format!("{}/api/generate", self.base_url);
        
        // Sending an empty prompt with an infinite keep_alive loads the model
        let payload = serde_json::json!({
            "model": model_name,
            "prompt": "",
            "keep_alive": -1 
        });

        self.client.post(&url).json(&payload).send().await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        Ok(())
    }

    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError> {
        let url = format!("{}/api/pull", self.base_url);
        
        // stream: false means Ollama will hold the connection open until the download finishes
        let payload = serde_json::json!({
            "name": model_name,
            "stream": false 
        });

        // We use a custom client here with no timeout because downloading a 4GB model takes time!
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3600)) // 1 hour timeout
            .build()
            .unwrap();

        let res = client.post(&url).json(&payload).send().await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if res.status().is_success() {
            Ok(())
        } else {
            Err(DriverError::ApiError(format!("Failed to install model: {}", res.status())))
        }
    }

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError> {
        let url = format!("{}/api/tags", self.base_url);
        let res = self.client.get(&url).send().await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(DriverError::ApiError(format!("Failed to fetch tags: {}", res.status())));
        }

        let data: OllamaTagsResponse = res.json().await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        let models = data.models.into_iter().map(|m| LocalModel {
            name: m.name,
            size_bytes: m.size,
            // Ollama returns a long ISO date, we just take the first 10 chars (YYYY-MM-DD)
            modified_at: m.modified_at.chars().take(10).collect(),
        }).collect();

        Ok(models)
    }
}