use crate::ipc::MemoryChunk;
use candle_core::{Device, Tensor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::Arc;

// This works across ALL models (Llama, Qwen, Mistral).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
}

// THE OS PAGEFILE MANAGER (SSD Swap)
pub struct Pager;

impl Pager {
    pub fn get_swap_dir() -> std::path::PathBuf {
        let path = crate::get_ore_dir().join("memory");
        if !path.exists() {
            std::fs::create_dir_all(&path).expect("Failed to create SSD Swap directory");
        }
        path
    }

    /// Generates a mathematically stable SHA-256 fingerprint of the JSON history
    pub fn get_history_fingerprint(app_id: &str) -> String {
        let history = Self::page_in_history(app_id);
        let mut hasher = Sha256::new();
        for msg in history {
            hasher.update(msg.role.as_bytes());
            hasher.update(msg.content.as_bytes());
        }
        let result = hasher.finalize();

        // Safely convert the 32 raw bytes into a 64-character hex string
        result.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Tier 1 Paging, Freeze the Agent's Chat History to the SSD
    pub fn page_out_history(app_id: &str, history: &Vec<ContextMessage>) {
        let swap_dir = Self::get_swap_dir();
        let path = swap_dir.join(format!("{}.json", app_id));

        if let Ok(data) = serde_json::to_string_pretty(history) {
            let _ = fs::write(&path, data);
            kprintln!("-> [PAGER] Agent '{}' history paged OUT to SSD.", app_id);
        }
    }

    /// Stream the Agent's Chat History from the SSD back into RAM
    pub fn page_in_history(app_id: &str) -> Vec<ContextMessage> {
        let swap_dir = Self::get_swap_dir();
        let path = swap_dir.join(format!("{}.json", app_id));

        if Path::new(&path).exists()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(history) = serde_json::from_str::<Vec<ContextMessage>>(&data)
        {
            kprintln!("-> [PAGER] Agent '{}' history paged IN from SSD.", app_id);
            return history;
        }
        Vec::new()
    }

    pub fn page_out_semantic(pipe_name: &str, chunks: &VecDeque<Arc<MemoryChunk>>) {
        let swap_dir = Self::get_swap_dir();
        let path = swap_dir.join(format!("{}.pipe", pipe_name));

        // Bincode freezes the RAM structure into pure 1s and 0s instantly
        if let Ok(data) = bincode::serialize(chunks) {
            let _ = fs::write(&path, data);
            kprintln!(
                "-> [PAGER] Semantic Pipe '{}' flushed to SSD (.pipe).",
                pipe_name
            );
        }
    }

    pub fn page_in_semantic(pipe_name: &str) -> Option<VecDeque<Arc<MemoryChunk>>> {
        let swap_dir = Self::get_swap_dir();
        let path = swap_dir.join(format!("{}.pipe", pipe_name));

        if Path::new(&path).exists() {
            // Read raw bytes instead of strings
            if let Ok(data) = fs::read(&path) {
                if let Ok(chunks) = bincode::deserialize::<VecDeque<Arc<MemoryChunk>>>(&data) {
                    kprintln!(
                        "-> [PAGER] Semantic Pipe '{}' mapped IN from SSD.",
                        pipe_name
                    );
                    return Some(chunks);
                } else {
                    kprintln!(
                        "-> [PAGER] [ERROR] Failed to deserialize pipe '{}'. The binary file might be corrupt or from an older version.",
                        pipe_name
                    );
                }
            }
        }
        None
    }

    pub fn page_out_kv_cache(
        app_id: &str,
        model_name: &str,
        tensors: &HashMap<String, Tensor>,
        fingerprint: &str,
    ) {
        let swap_dir = Self::get_swap_dir();
        let safe_model = model_name.replace(":", "-");

        let tensor_path = swap_dir.join(format!("{}_{}.safetensors", app_id, safe_model));
        let hash_path = swap_dir.join(format!("{}_{}.hash", app_id, safe_model));

        // Save the raw math matrices directly to the SSD
        if let Err(e) = candle_core::safetensors::save(tensors, &tensor_path) {
            kprintln!("-> [PAGER] [ERROR] Failed to save KV-Cache to SSD: {}", e);
        } else {
            let _ = fs::write(&hash_path, fingerprint);
            kprintln!(
                "-> [PAGER] Agent '{}' KV-Cache ({} Tensors) paged OUT to SSD.",
                app_id,
                tensors.len()
            );
        }
    }

    pub fn page_in_kv_cache(
        app_id: &str,
        model_name: &str,
        device: &Device,
        current_fingerprint: &str,
    ) -> Option<HashMap<String, Tensor>> {
        let safe_model = model_name.replace(":", "-");
        let swap_dir = Self::get_swap_dir();
        let tensor_path = swap_dir.join(format!("{}_{}.safetensors", app_id, safe_model));
        let hash_path = swap_dir.join(format!("{}_{}.hash", app_id, safe_model));

        if Path::new(&hash_path).exists()
            && let Ok(saved_hash) = fs::read_to_string(&hash_path)
        {
            if saved_hash == current_fingerprint {
                if Path::new(&tensor_path).exists() {
                    match candle_core::safetensors::load(&tensor_path, device) {
                        Ok(tensors) => {
                            kprintln!("-> [PAGER] Agent '{}' KV-Cache paged IN from SSD.", app_id);
                            return Some(tensors);
                        }
                        Err(e) => {
                            kprintln!(
                                "-> [PAGER] [WARN] Failed to load KV-Cache: {}. Falling back to JSON History.",
                                e
                            );
                        }
                    }
                }
            } else {
                println!(
                    "-> [PAGER] Context Fingerprint MISMATCH. History was altered. Forcing Cold Start."
                );
            }
        }
        None
    }

    pub fn get_kv_cache_size_mb(app_id: &str, model_name: &str) -> u32 {
        let safe_model = model_name.replace(":", "-");
        let swap_dir = Self::get_swap_dir();
        let path = swap_dir.join(format!("{}_{}.safetensors", app_id, safe_model));

        if let Ok(metadata) = fs::metadata(&path) {
            (metadata.len() / (1024 * 1024)) as u32
        } else {
            0
        }
    }

    /// Wipe the memory clean
    pub fn clear_page(app_id: &str) {
        let swap_dir = Self::get_swap_dir();
        let _ = fs::remove_file(swap_dir.join(format!("{}.json", app_id)));
        let _ = fs::remove_file(swap_dir.join(format!("{}.pipe", app_id)));

        // Sweep for any Model-Specific Safetensor KV-Caches
        if let Ok(entries) = fs::read_dir(&swap_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.starts_with(&format!("{}_", app_id))
                    && (file_name.ends_with(".safetensors") || file_name.ends_with(".hash"))
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        kprintln!(
            "-> [PAGER] Completely wiped all swap files for Agent '{}'",
            app_id
        );
    }

    pub fn delete_kv_cache(app_id: &str) {
        let swap_dir = Self::get_swap_dir();
        if let Ok(entries) = fs::read_dir(&swap_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.starts_with(&format!("{}_", app_id))
                    && (file_name.ends_with(".safetensors") || file_name.ends_with(".hash"))
                {
                    let _ = fs::remove_file(entry.path());
                    kprintln!(
                        "-> [PAGER] Deleted stale KV-Cache for '{}' (Memory Compaction).",
                        app_id
                    );
                }
            }
        }
    }
}
