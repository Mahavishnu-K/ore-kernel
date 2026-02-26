use serde::Deserialize;
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

// app resgistry structure and loader
#[derive(Deserialize, Debug, Clone)]
pub struct AppManifest {
    pub app_id: String,
    pub permissions: Permissions,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Permissions {
    pub can_read_files: bool,
    pub can_access_internet: bool,
    pub max_tokens_per_request: Option<u32>,
}

// the app registry (In-Memory Cache)
#[derive(Debug, Clone)]
pub struct AppRegistry {
    /// Maps an `app_id` to its verified `AppManifest`
    apps: HashMap<String, AppManifest>,
}

impl AppRegistry {
    /// Sweeps a directory on boot, loading all .toml files into RAM
    pub fn boot_load(manifests_dir: &str) -> Result<Self, RegistryError> {
        let mut apps = HashMap::new();
        let path = Path::new(manifests_dir);

        if !path.exists() {
            fs::create_dir_all(path).map_err(|e| RegistryError::IoError(e.to_string()))?;
            println!("-> [REGISTRY] Created new manifests directory at {}", manifests_dir);
            return Ok(Self { apps });
        }

        for entry in fs::read_dir(path).map_err(|e| RegistryError::IoError(e.to_string()))? {
            let entry = entry.map_err(|e| RegistryError::IoError(e.to_string()))?;
            let file_path = entry.path();

            if file_path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let toml_string = fs::read_to_string(&file_path)
                    .map_err(|e| RegistryError::IoError(e.to_string()))?;
                
                let manifest: AppManifest = toml::from_str(&toml_string)
                    .map_err(|e| RegistryError::ParseError(
                        file_path.display().to_string(), 
                        e.to_string()
                    ))?;

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
}