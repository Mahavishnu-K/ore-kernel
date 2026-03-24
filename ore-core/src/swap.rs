use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

// This works across ALL models (Llama, Qwen, Mistral).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
}

// THE OS PAGEFILE MANAGER (SSD Swap)
pub struct Pager;

impl Pager {
    const SWAP_DIR: &'static str = "../swap";

    pub fn ensure_swap_drive() {
        if !Path::new(Self::SWAP_DIR).exists() {
            fs::create_dir_all(Self::SWAP_DIR).expect("Failed to create SSD Swap directory");
        }
    }

    /// Tier 1 Paging, Freeze the Agent's Chat History to the SSD
    pub fn page_out_history(app_id: &str, history: &Vec<ContextMessage>) {
        Self::ensure_swap_drive();
        let path = format!("{}/{}.json", Self::SWAP_DIR, app_id);

        if let Ok(data) = serde_json::to_string_pretty(history) {
            let _ = fs::write(&path, data);
            println!("-> [PAGER] Agent '{}' history paged OUT to SSD.", app_id);
        }
    }

    /// Stream the Agent's Chat History from the SSD back into RAM
    pub fn page_in_history(app_id: &str) -> Vec<ContextMessage> {
        let path = format!("{}/{}.json", Self::SWAP_DIR, app_id);

        if Path::new(&path).exists()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(history) = serde_json::from_str::<Vec<ContextMessage>>(&data)
        {
            println!("-> [PAGER] Agent '{}' history paged IN from SSD.", app_id);
            return history;
        }
        Vec::new()
    }

    /// Wipe the memory clean
    pub fn clear_page(app_id: &str) {
        let path_json = format!("{}/{}.json", Self::SWAP_DIR, app_id);
        let path_bin = format!("{}/{}.bin", Self::SWAP_DIR, app_id);

        let _ = fs::remove_file(path_json);
        let _ = fs::remove_file(path_bin);
    }
}
