//! Common protocol types, API payloads, and DTOs shared between ORE Server and CLI.

use serde::{Deserialize, Serialize};

// ─── Telemetry Models ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HostTelemetry {
    pub os_name: String,
    pub os_version: String,
    pub host_name: String,
    pub cpu_count: usize,
    pub cpu_brand: String,
    pub cpu_usage_pct: f32,
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub free_memory_mb: u64,
    pub total_swap_mb: u64,
    pub used_swap_mb: u64,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PsResponse {
    pub models: Vec<ModelProcessInfo>,
    pub total_ram_mb: u64,
    pub total_vram_mb: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopTelemetryResponse {
    pub host: HostTelemetry,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalModelInfo {
    pub name: String,
    pub format: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub is_loaded: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LsResponse {
    pub models: Vec<LocalModelInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentsResponse {
    pub agents: Vec<AgentTelemetryInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManifestTelemetryInfo {
    pub file_name: String,
    pub app_id: String,
    pub network: String,
    pub file_io: String,
    pub execution: String,
    pub pii_scrubbing: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManifestsResponse {
    pub manifests: Vec<ManifestTelemetryInfo>,
}

// ─── Execution & IPC Payloads ────────────────────────────────────────────────

// ─── Vector Modes & RAPTOR DTOs ─────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorMode {
    #[default]
    Auto,
    HotSimd,
    ColdLeann,
    DiskAnnNvme,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RaptorBuildRequest {
    pub app_id: String,
    pub target_pipe: String,
    pub cluster_size: Option<usize>,
    pub max_levels: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RaptorBuildResponse {
    pub target_pipe: String,
    pub generated_summaries: usize,
    pub tree_levels: usize,
}

#[repr(u8)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    /// L1: Episodic truths, logs, observations, and context
    Fact = 0,
    /// L2: Procedural workflows, code routines, tool usage rules, and SOPs
    Skill = 1,
    /// L3: Permanent constraints, persona guidelines, and system directives
    Directive = 2,
    /// Default unclassified knowledge
    #[default]
    General = 3,
}

/// Body of `POST /infer`: one stateful agent turn.
/// The agent is identified by the `X-App-ID` header, not by the body.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InferRequest {
    pub prompt: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RunRequest {
    pub app_id: String,
    pub model: String,
    pub prompt: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChunkStrategy {
    #[default]
    SlidingWindow,
    SentenceAware,
    SentenceWindow,
    Paragraph,
    ExactMatch,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IpcShareRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub knowledge_text: String,
    pub chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
    pub chunk_strategy: Option<ChunkStrategy>,
    #[serde(default)]
    pub category: Option<MemoryCategory>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IpcSearchRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub query: String,
    pub filter_app: Option<String>,
    #[serde(default)]
    pub filter_category: Option<MemoryCategory>,
    pub top_k: Option<usize>,
    #[serde(default)]
    pub enable_hybrid: Option<bool>,
    #[serde(default)]
    pub confidence_threshold: Option<f32>,
    #[serde(default)]
    pub crag_fallback_tool: Option<String>,
    #[serde(default)]
    pub enable_hyde: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SearchResult {
    pub text: String,
    pub context_payload: String,
    pub score: f32,
    pub source_app: String,
    pub timestamp: u64,
    #[serde(default)]
    pub category: MemoryCategory,
    #[serde(default)]
    pub is_crag_fallback: bool,
}

// ─── CoW Branching DTOs ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StagedChunk {
    pub id: u64,
    pub text: String,
    pub context_payload: String,
    pub source_app: String,
    pub timestamp: u64,
    pub category: MemoryCategory,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PipeBranchRequest {
    pub app_id: String,
    pub base_pipe: String,
    pub branch_pipe: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PipeDiffRequest {
    pub app_id: String,
    pub base_pipe: String,
    pub branch_pipe: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PipeDiffResponse {
    pub base_pipe: String,
    pub branch_pipe: String,
    pub delta_count: usize,
    pub staged_chunks: Vec<StagedChunk>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PipeMergeRequest {
    pub app_id: String,
    pub base_pipe: String,
    pub branch_pipe: String,
    #[serde(default)]
    pub delete_branch_after_merge: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecuteRequest {
    pub app_id: String,
    pub tool_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub input_data: Option<String>,
    pub language: Option<String>,
    pub script: Option<String>,
    pub dependencies: Option<Vec<String>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
