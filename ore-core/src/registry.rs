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

pub use ore_common::manifest::*;

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
                // A broken manifest only disables its own agent; it never stops the kernel.
                let toml_string = match fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(e) => {
                        let err = RegistryError::IoError(e.to_string());
                        kprintln!(
                            "-> [SECURITY ALERT] Skipping manifest '{}'.",
                            file_path.display()
                        );
                        kprintln!("   KERNEL ERROR: {}", err);
                        continue;
                    }
                };

                let manifest: AppManifest = match toml::from_str(&toml_string) {
                    Ok(m) => m,
                    Err(e) => {
                        let err = RegistryError::ParseError(
                            file_path.display().to_string(),
                            e.to_string(),
                        );
                        kprintln!(
                            "-> [SECURITY ALERT] Skipping manifest '{}'.",
                            file_path.display()
                        );
                        kprintln!("   KERNEL ERROR: {}", err);
                        continue;
                    }
                };

                if let Err(e) = manifest.validate() {
                    kprintln!(
                        "-> [SECURITY ALERT] Failed to load manifest for '{}'.",
                        manifest.app_id
                    );
                    kprintln!("   KERNEL ERROR: {}", e);
                    continue;
                }

                if apps.contains_key(&manifest.app_id) {
                    kprintln!(
                        "-> [SECURITY ALERT] Duplicate app_id '{}' in '{}'. Keeping the first one loaded.",
                        manifest.app_id,
                        file_path.display()
                    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_manifests_are_skipped_not_fatal() {
        let dir = std::env::temp_dir().join(format!("ore_registry_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let good = "app_id = \"good\"\ndescription = \"\"\nversion = \"1\"\n";
        fs::write(dir.join("good.toml"), good).unwrap();
        fs::write(dir.join("broken.toml"), "app_id = \"broken\"\n[[[ not toml").unwrap();
        fs::write(
            dir.join("invalid.toml"),
            "app_id = \"\"\ndescription = \"\"\nversion = \"1\"\n",
        )
        .unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let registry = AppRegistry::boot_load(dir.to_str().unwrap()).expect("boot must not fail");
        assert!(registry.get_app("good").is_some());
        assert_eq!(registry.list_apps().len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
