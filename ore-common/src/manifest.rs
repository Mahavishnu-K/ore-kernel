//! Agent Manifest specification, permission models, and validation logic.

use crate::constants::{
    DEFAULT_FUEL_LIMIT, DEFAULT_MAX_JSON_TOKENS, DEFAULT_MAX_KV_CACHE_MB,
    DEFAULT_MAX_TOKENS_PER_MINUTE,
};
use serde::{Deserialize, Serialize};

/// Master Agent Manifest schema representing zero-trust capability permissions.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AppManifest {
    pub app_id: String,
    pub description: String,
    pub version: String,

    #[serde(default)]
    pub privacy: Privacy,
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub memory_limits: MemoryLimits,
    #[serde(default)]
    pub file_system: FileSystem,
    #[serde(default)]
    pub network: Network,
    #[serde(default)]
    pub execution: Execution,
    #[serde(default)]
    pub ipc: Ipc,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Privacy {
    pub enforce_pii_redaction: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Resources {
    pub allowed_models: Vec<String>,
    pub max_tokens_per_minute: u32,
    pub gpu_priority: String,

    #[serde(default = "default_true")]
    pub json_history: bool,

    #[serde(default = "default_false")]
    pub stateful_paging: bool,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            allowed_models: Vec::new(),
            max_tokens_per_minute: DEFAULT_MAX_TOKENS_PER_MINUTE,
            gpu_priority: "normal".to_string(),
            json_history: true,
            stateful_paging: false,
        }
    }
}

fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MemoryLimits {
    /// The maximum conversational context before the AI gets confused (e.g., 8192 tokens).
    pub max_json_tokens: u32,

    /// The physical SSD/VRAM size limit for the frozen brain state (e.g., 1024 MB = 1 GB).
    pub max_kv_cache_mb: u32,

    /// If either of the above limits are hit, should ORE summarize the history?
    pub auto_summarize_on_cap: bool,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            max_json_tokens: DEFAULT_MAX_JSON_TOKENS,
            max_kv_cache_mb: DEFAULT_MAX_KV_CACHE_MB,
            auto_summarize_on_cap: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSystem {
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Network {
    pub network_enabled: bool,
    pub allow_localhost_access: bool,
    #[serde(default)]
    pub rules: Vec<NetworkRule>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NetworkRule {
    pub domain: String,
    pub allowed_methods: Vec<String>,
    pub allowed_paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    #[serde(default)]
    pub can_execute_shell: bool,
    #[serde(default = "default_true")]
    pub can_execute_wasm: bool,
    #[serde(default)]
    pub is_coding_agent: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_language_runtimes: Vec<String>,

    #[serde(default = "default_cpu_instructions")]
    pub max_cpu_instructions: u64,
}

impl Default for Execution {
    fn default() -> Self {
        Self {
            can_execute_shell: false,
            can_execute_wasm: true,
            is_coding_agent: false,
            allowed_tools: Vec::new(),
            allowed_language_runtimes: Vec::new(),
            max_cpu_instructions: DEFAULT_FUEL_LIMIT,
        }
    }
}

fn default_cpu_instructions() -> u64 {
    DEFAULT_FUEL_LIMIT
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Ipc {
    #[serde(default)]
    pub allowed_agent_targets: Vec<String>,

    #[serde(default)]
    pub allowed_semantic_pipes: Vec<String>,

    #[serde(default = "default_true")]
    pub semantic_persistence: bool,

    #[serde(default = "default_false")]
    pub allow_time_decay: bool,
}

impl Default for Ipc {
    fn default() -> Self {
        Self {
            allowed_agent_targets: Vec::new(),
            allowed_semantic_pipes: Vec::new(),
            semantic_persistence: true,
            allow_time_decay: false,
        }
    }
}

impl AppManifest {
    /// Whether this agent may run `model`. `"*"` in `allowed_models` allows every model.
    /// Names match with `:` and `-` treated alike (`qwen2.5:0.5b` == `qwen2.5-0.5b`),
    /// since that's how models are stored on disk.
    pub fn allows_model(&self, model: &str) -> bool {
        let wanted = model.trim().replace(':', "-");
        self.resources
            .allowed_models
            .iter()
            .any(|m| m == "*" || m.trim().replace(':', "-") == wanted)
    }

    /// The first concrete model in `allowed_models` (skipping the `"*"` wildcard).
    pub fn default_model(&self) -> Option<&str> {
        self.resources
            .allowed_models
            .iter()
            .map(|m| m.as_str())
            .find(|m| *m != "*")
    }

    /// Validates core invariant rules for the manifest.
    pub fn validate(&self) -> Result<(), String> {
        if self.app_id.trim().is_empty() {
            return Err("FATAL: 'app_id' cannot be empty.".to_string());
        }

        // RULE 1: The Immutable Anchor
        if self.resources.stateful_paging && !self.resources.json_history {
            return Err("FATAL: 'stateful_paging' cannot be true if 'json_history' is false. \
                        ORE requires JSON fallbacks to prevent KV-Cache corruption and to perform memory compaction.".to_string());
        }

        // RULE 2: Non-Zero Budgets
        if self.resources.max_tokens_per_minute == 0 {
            return Err(
                "FATAL: 'max_tokens_per_minute' cannot be 0. Agent would be permanently frozen."
                    .to_string(),
            );
        }

        if self.resources.stateful_paging && self.memory_limits.max_kv_cache_mb == 0 {
            return Err("FATAL: 'max_kv_cache_mb' cannot be 0. Give the agent at least some physical VRAM space.".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_manifest() {
        let mut manifest = AppManifest {
            app_id: "test_agent".to_string(),
            ..Default::default()
        };
        manifest.resources.max_tokens_per_minute = 1000;
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_empty_app_id_invalid() {
        let manifest = AppManifest::default();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_allows_model() {
        let mut manifest = AppManifest::default();
        manifest.resources.allowed_models = vec!["qwen2.5:0.5b".to_string()];
        assert!(manifest.allows_model("qwen2.5:0.5b"));
        assert!(manifest.allows_model("qwen2.5-0.5b"));
        assert!(!manifest.allows_model("llama3.2:1b"));

        manifest.resources.allowed_models.clear();
        assert!(!manifest.allows_model("qwen2.5:0.5b"));

        manifest.resources.allowed_models = vec!["*".to_string()];
        assert!(manifest.allows_model("anything:7b"));
    }

    #[test]
    fn test_default_model_skips_wildcard() {
        let mut manifest = AppManifest::default();
        manifest.resources.allowed_models = vec!["*".to_string(), "llama3.2:1b".to_string()];
        assert_eq!(manifest.default_model(), Some("llama3.2:1b"));

        manifest.resources.allowed_models = vec!["*".to_string()];
        assert_eq!(manifest.default_model(), None);
    }

    #[test]
    fn test_paging_without_json_invalid() {
        let mut manifest = AppManifest {
            app_id: "agent_x".to_string(),
            ..Default::default()
        };
        manifest.resources.stateful_paging = true;
        manifest.resources.json_history = false;
        assert!(manifest.validate().is_err());
    }
}
