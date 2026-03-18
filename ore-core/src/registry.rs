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

    #[serde(default)] 
    pub stateful_paging: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FileSystem {
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
    pub max_file_size_mb: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Network {
    pub network_enabled: bool,
    pub allowed_domains: Vec<String>,
    pub allow_localhost_access: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Execution {
    pub can_execute_shell: bool,
    pub can_execute_wasm: bool,
    pub allowed_tools: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Ipc {
    #[serde(default)]
    pub allowed_agent_targets: Vec<String>,

    #[serde(default)]
    pub allowed_semantic_pipes: Vec<String>,
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
            println!(
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

                println!("-> [REGISTRY] Verified & Loaded App: {}", manifest.app_id);
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
