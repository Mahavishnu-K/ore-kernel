use crate::driver::{DriverError, InferenceDriver, LocalModel, VramProcess};
use crate::swap::ContextMessage;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;

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

#[derive(serde::Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<ContextMessage>,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: ContextMessage,
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

#[derive(serde::Serialize)]
struct OllamaEmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(serde::Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[async_trait]
impl InferenceDriver for OllamaDriver {
    fn engine_name(&self) -> &'static str {
        "Ollama Engine"
    }

    async fn is_online(&self) -> bool {
        self.client.get(&self.base_url).send().await.is_ok()
    }

    // This scans Ollama's RAM/VRAM
    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError> {
        let url = format!("{}/api/ps", self.base_url);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(DriverError::ApiError(format!(
                "Ollama returned {}",
                res.status()
            )));
        }

        let data: OllamaPsResponse = res
            .json()
            .await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        // Translate Ollama's JSON into ORE's standard Process list
        let processes = data
            .models
            .into_iter()
            .map(|m| VramProcess {
                model_name: m.name,
                size_bytes: m.size,
                size_vram_bytes: m.size_vram,
            })
            .collect();

        Ok(processes)
    }

    async fn generate_text(
        &self,
        model: &str,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
    ) -> Result<(), DriverError> {
        let url = format!("{}/api/chat", self.base_url);

        let mut messages = history.unwrap_or_default();

        messages.push(ContextMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        let payload = OllamaRequest {
            model: model.to_string(),
            messages,
            stream: false,
        };

        let res = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        let data: OllamaResponse = res
            .json()
            .await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        let _ = tx.send(data.message.content);
        Ok(())
    }

    async fn unload_model(&self, model_name: &str) -> Result<(), DriverError> {
        let url = format!("{}/api/generate", self.base_url);

        // Setting keep_alive to 0 tells the driver to drop it from RAM
        let payload = serde_json::json!({
            "model": model_name,
            "keep_alive": 0
        });

        let res = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if res.status().is_success() {
            Ok(())
        } else {
            Err(DriverError::ApiError(format!(
                "Failed to unload: {}",
                res.status()
            )))
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

        self.client
            .post(&url)
            .json(&payload)
            .send()
            .await
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

        let res = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if res.status().is_success() {
            Ok(())
        } else {
            Err(DriverError::ApiError(format!(
                "Failed to install model: {}",
                res.status()
            )))
        }
    }

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError> {
        let url = format!("{}/api/tags", self.base_url);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(DriverError::ApiError(format!(
                "Failed to fetch tags: {}",
                res.status()
            )));
        }

        let data: OllamaTagsResponse = res
            .json()
            .await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        let models = data
            .models
            .into_iter()
            .map(|m| LocalModel {
                name: m.name,
                size_bytes: m.size,
                modified_at: m.modified_at.chars().take(10).collect(),
            })
            .collect();

        Ok(models)
    }

    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError> {
        let url = format!("{}/api/embed", self.base_url);

        let payload = OllamaEmbedRequest {
            model: model.to_string(),
            input: inputs,
        };

        let res = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(DriverError::ApiError(format!(
                "Ollama error: {}",
                res.status()
            )));
        }

        let data: OllamaEmbedResponse = res
            .json()
            .await
            .map_err(|e| DriverError::ApiError(e.to_string()))?;

        Ok(data.embeddings)
    }
}
