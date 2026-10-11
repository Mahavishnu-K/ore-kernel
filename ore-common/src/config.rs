//! ore.toml system configuration schema and defaults.

use crate::constants::DEFAULT_EMBEDDER;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration schema parsed from ore.toml.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct OreConfig {
    pub system: SystemConfig,
    pub memory: MemoryConfig,
    #[serde(default)]
    pub models: HashMap<String, ModelConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ModelConfig {
    pub kv_cache_mb_per_1k: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemConfig {
    pub engine: String,

    #[serde(default = "default_embedder")]
    pub embedder: String,
}

fn default_embedder() -> String {
    DEFAULT_EMBEDDER.to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryConfig {
    pub cache_ttl_hours: u64,
    pub pipe_ttl_hours: u64,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            engine: "native".to_string(),
            embedder: default_embedder(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            cache_ttl_hours: 24,
            pipe_ttl_hours: 24,
        }
    }
}
