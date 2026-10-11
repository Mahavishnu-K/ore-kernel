//! Shared core domain models and telemetry data structures across ORE.

use serde::{Deserialize, Serialize};

/// Universal chat/prompt message structure used across models and memory systems.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
}

impl ContextMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }
}

/// Metadata describing a model stored on the local disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalModel {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
    #[serde(default)]
    pub format: String,
}

/// In-flight process telemetry representing an actively loaded model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VramProcess {
    pub model_name: String,
    pub size_bytes: u64,
    pub size_vram_bytes: u64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub active_requests: usize,
    #[serde(default)]
    pub last_used_secs: u64,
}
