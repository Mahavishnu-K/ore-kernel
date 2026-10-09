use serde::Deserialize;

#[derive(serde::Deserialize)]
pub struct RunRequest {
    pub app_id: String,
    pub model: String,
    pub prompt: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChunkStrategy {
    #[default]
    SlidingWindow,
    SentenceAware,
    Paragraph,
    ExactMatch,
}

#[derive(serde::Deserialize)]
pub struct IpcShareRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub knowledge_text: String,
    pub chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
    pub chunk_strategy: Option<ChunkStrategy>,
}

#[derive(serde::Deserialize)]
pub struct IpcSearchRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub query: String,
    pub filter_app: Option<String>,
    pub top_k: Option<usize>,
}

#[derive(serde::Serialize)]
pub struct SearchResult {
    pub text: String,
    pub score: f32,
    pub source_app: String,
    pub timestamp: u64,
}

#[derive(serde::Deserialize)]
pub struct ExecuteRequest {
    pub app_id: String,

    // Fixed Tool Mode ("Console Cartridge")
    pub tool_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub input_data: Option<String>, // Allow passing complex JSON/Text into the tool via STDIN

    // Autonomous Mode ("Inception")
    pub language: Option<String>,
    pub script: Option<String>,
    pub dependencies: Option<Vec<String>>,

    // Native Shell Mode (Ring 2 / Host)
    pub shell_command: Option<String>,
}

impl ExecuteRequest {
    pub fn execution_mode(&self) -> Result<ExecutionMode, &'static str> {
        let has_tool = self.tool_name.is_some();
        let has_script = self.script.is_some();
        let has_shell = self.shell_command.is_some();

        match has_tool as u8 + has_script as u8 + has_shell as u8 {
            0 => Err("Empty execution request. Specify tool_name, script, or shell_command."),
            1 => {
                if let Some(tool) = &self.tool_name {
                    Ok(ExecutionMode::Tool {
                        name: tool.clone(),
                        args: self.args.clone().unwrap_or_default(),
                        input_data: self.input_data.clone(),
                    })
                } else if let Some(script) = &self.script {
                    Ok(ExecutionMode::Script {
                        language: self
                            .language
                            .clone()
                            .unwrap_or_else(|| "python".to_string()),
                        script: script.clone(),
                        dependencies: self.dependencies.clone().unwrap_or_default(),
                        input_data: self.input_data.clone(),
                    })
                } else if let Some(cmd) = &self.shell_command {
                    Ok(ExecutionMode::Shell {
                        command: cmd.clone(),
                    })
                } else {
                    unreachable!()
                }
            }
            _ => Err("Ambiguous request: choose exactly one mode (tool, script, or shell)."),
        }
    }
}

pub enum ExecutionMode {
    Tool {
        name: String,
        args: Vec<String>,
        input_data: Option<String>,
    },
    Script {
        language: String,
        script: String,
        dependencies: Vec<String>,
        input_data: Option<String>,
    },
    Shell {
        command: String,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct KernelHealthResponse {
    pub status: String,
    pub version: String,
    pub engine: String,
    pub device: String,
    pub ore_dir: String,
    pub loaded_models: usize,
    pub registered_agents: usize,
    pub uptime_seconds: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ModelProcessInfo {
    pub model_name: String,
    pub engine: String,
    pub device: String,
    pub status: String,
    pub active_requests: usize,
    pub host_ram_mb: u64,
    pub gpu_vram_mb: u64,
    pub last_used_secs: u64,
    pub current_app_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct PsResponse {
    pub models: Vec<ModelProcessInfo>,
    pub total_ram_mb: u64,
    pub total_vram_mb: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct TopTelemetryResponse {
    pub host: ore_core::telemetry::HostTelemetry,
    pub accelerator_device: String,
    pub vram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_free_mb: u64,
    pub vram_reserved_mb: u64,
    pub vram_safety_margin_mb: u64,
    pub engine_name: String,
    pub scheduler_summary: String,
    pub active_models_count: usize,
    pub registered_agents_count: usize,
    pub context_firewall: String,
    pub semantic_bus_cache_count: usize,
    pub wasm_sandbox: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct LocalModelInfo {
    pub name: String,
    pub format: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub is_loaded: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct LsResponse {
    pub models: Vec<LocalModelInfo>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AgentTelemetryInfo {
    pub app_id: String,
    pub version: String,
    pub allowed_models: Vec<String>,
    pub priority: String,
    pub network_enabled: bool,
    pub filesystem_permissions: String,
    pub execution_engine: String,
    pub pii_enforcement: bool,
    pub status: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AgentsResponse {
    pub agents: Vec<AgentTelemetryInfo>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ManifestTelemetryInfo {
    pub file_name: String,
    pub app_id: String,
    pub network: String,
    pub file_io: String,
    pub execution: String,
    pub pii_scrubbing: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ManifestsResponse {
    pub manifests: Vec<ManifestTelemetryInfo>,
}
