use serde::Deserialize;

#[derive(serde::Deserialize)]
pub struct RunRequest {
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
