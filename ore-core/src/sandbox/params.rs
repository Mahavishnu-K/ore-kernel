use crate::registry::NetworkRule;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ExecuteParams {
    pub tool_name: String,
    pub wasm_binary: Vec<u8>,
    pub fuel_limit: u64,
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub inception: Option<(String, String)>,
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
    pub network_enabled: bool,
    pub allow_localhost_access: bool,
    pub network_rules: Vec<NetworkRule>,
    pub dynamic_vfs_path: Option<String>,
    pub wasm_path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OreResponseMeta {
    pub status: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
}
