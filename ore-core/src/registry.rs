use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Failed to read manifest directory: {0}")]
    IoError(String),
    #[error("Failed to parse manifest TOML '{0}': {1}")]
    ParseError(String, String),
}

// app resgistry manifest structure
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Privacy {
    pub enforce_pii_redaction: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Resources {
    pub allowed_models: Vec<String>,
    pub max_tokens_per_minute: u32,
    pub gpu_priority: String,

    #[serde(default = "default_true")]
    pub json_history: bool,

    #[serde(default = "default_false")]
    pub stateful_paging: bool,
}

fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryLimits {
    /// The maximum conversational context before the AI gets confused (e.g., 8192 tokens)
    /// We use a rough heuristic: 1 token ~= 4 characters of JSON text.
    pub max_json_tokens: u32,

    /// The physical SSD/VRAM size limit for the frozen brain state (e.g., 1024 MB = 1 GB)
    pub max_kv_cache_mb: u32,

    /// If either of the above limits are hit, should ORE summarize the history?
    pub auto_summarize_on_cap: bool,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            max_json_tokens: 8192,
            max_kv_cache_mb: 1024,
            auto_summarize_on_cap: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FileSystem {
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Network {
    pub network_enabled: bool,
    pub allow_localhost_access: bool,
    #[serde(default)]
    pub rules: Vec<NetworkRule>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkRule {
    pub domain: String,
    pub allowed_methods: Vec<String>,
    pub allowed_paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Execution {
    #[serde(default)]
    pub can_execute_shell: bool,
    #[serde(default)]
    pub can_execute_wasm: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_language_runtimes: Vec<String>,

    #[serde(default = "default_cpu_instructions")]
    pub max_cpu_instructions: u64,
}

// Manually implement Default so new empty structs get the 5 Billion baseline!
impl Default for Execution {
    fn default() -> Self {
        Self {
            can_execute_shell: false,
            can_execute_wasm: true,
            allowed_tools: Vec::new(),
            allowed_language_runtimes: Vec::new(),
            max_cpu_instructions: 5_000_000_000,
        }
    }
}

fn default_cpu_instructions() -> u64 {
    5_000_000_000 // 5 Billion Instructions (roughly 2-3 seconds of raw compute)
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
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

// the app registry (In-Memory Cache)
#[derive(Debug, Clone)]
pub struct AppRegistry {
    apps: HashMap<String, AppManifest>,
}

impl AppRegistry {
    /// Sweeps a directory on boot, loading all .toml files into RAM
    pub fn boot_load(manifests_dir: &str) -> Result<Self, RegistryError> {
        let mut apps = HashMap::new();
        let path = Path::new(manifests_dir);

        if !path.exists() {
            fs::create_dir_all(path).map_err(|e| RegistryError::IoError(e.to_string()))?;
            kprintln!(
                "-> [REGISTRY] Created new manifests directory at {}",
                manifests_dir
            );
            return Ok(Self { apps });
        }

        for entry in fs::read_dir(path).map_err(|e| RegistryError::IoError(e.to_string()))? {
            let entry = entry.map_err(|e| RegistryError::IoError(e.to_string()))?;
            let file_path = entry.path();

            if file_path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let toml_string = fs::read_to_string(&file_path)
                    .map_err(|e| RegistryError::IoError(e.to_string()))?;

                let manifest: AppManifest = toml::from_str(&toml_string).map_err(|e| {
                    RegistryError::ParseError(file_path.display().to_string(), e.to_string())
                })?;

                if let Err(e) = manifest.validate() {
                    kprintln!(
                        "-> [SECURITY ALERT] Failed to load manifest for '{}'.",
                        manifest.app_id
                    );
                    kprintln!("   KERNEL ERROR: {}", e);
                    continue;
                }

                kprintln!("-> [REGISTRY] Verified & Loaded App: {}", manifest.app_id);
                apps.insert(manifest.app_id.clone(), manifest);
            }
        }

        Ok(Self { apps })
    }

    /// O(1) ultra-fast lookup for the Firewall
    pub fn get_app(&self, app_id: &str) -> Option<&AppManifest> {
        self.apps.get(app_id)
    }

    pub fn list_apps(&self) -> Vec<AppManifest> {
        self.apps.values().cloned().collect()
    }
}

impl AppManifest {
    fn validate(&self) -> Result<(), String> {
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

        // if self.resources.json_history && self.memory_limits.max_json_tokens < 500 {
        //     return Err("FATAL: 'max_context_tokens' must be at least 500. Otherwise the AI won't have enough memory to even generate a summary!".to_string());
        // }

        if self.resources.stateful_paging && self.memory_limits.max_kv_cache_mb == 0 {
            return Err("FATAL: 'max_kv_cache_mb' cannot be 0. Give the agent at least some physical VRAM space.".to_string());
        }

        Ok(())
    }
}
