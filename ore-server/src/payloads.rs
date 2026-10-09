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
