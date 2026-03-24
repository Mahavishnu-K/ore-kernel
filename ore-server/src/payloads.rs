#[derive(serde::Deserialize)]
pub struct RunRequest {
    pub model: String,
    pub prompt: String,
}

#[derive(serde::Deserialize)]
pub struct IpcShareRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub knowledge_text: String,
    pub chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
}

#[derive(serde::Deserialize)]
pub struct IpcSearchRequest {
    pub source_app: String,
    pub target_pipe: String,
    pub query: String,
    pub filter_app: Option<String>,
    pub top_k: Option<usize>,
}
