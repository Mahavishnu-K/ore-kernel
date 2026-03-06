use dashmap::DashMap;
use tokio::sync::broadcast;
use std::time::{Instant, Duration};

// Inter-process communication structures and utilities for ORE Agents
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentMessage {
    pub from_app: String,
    pub to_app: String,
    pub payload: String, 
    pub timestamp: u64,
}

pub struct MessageBus {
    channel: DashMap<String, broadcast::Sender<AgentMessage>>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            channel: DashMap::new(),
        }
    }

    /// App A sends a message to App B
    pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
        if let Some(target_channel) = self.channel.get(&msg.to_app) {
            let _ = target_channel.send(msg);
            Ok(())
        } else {
            Err(format!("Agent '{}' is not currently listening.", msg.to_app))
        }
    }

    /// App B registers itself to listen for messages
    pub fn register_listener(&self, app_id: &str) -> broadcast::Receiver<AgentMessage> {
        let sender = self.channel.entry(app_id.to_string()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(100);
            tx
        });
        sender.subscribe()
    }
}

// In-memory shared data pipes for semantic communication pipe
// Tier 2: The lazy semantic bus (System-Level Vector DB)
#[derive(Clone, Debug)]
pub struct MemoryChunk {
    pub text: String,
    pub vector: Vec<f32>,
}

pub struct SemanticBus {
    memory_pipes: DashMap<String, Vec<MemoryChunk>>,
}

impl SemanticBus {
    pub fn new() -> Self {
        Self {
            memory_pipes: DashMap::new(),
        }
    }

    pub fn write_chunk(&self, pipe_name: &str, text: String, vector: Vec<f32>) {
        let mut pipe = self.memory_pipes.entry(pipe_name.to_string()).or_insert_with(Vec::new);
        pipe.push(MemoryChunk { text, vector });
    }

    /// Core Vector Search Algorithm (Cosine Similarity)
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
    }

    /// Searches the pipe and returns the top 3 most relevant text chunks based on cosine similarity
    pub fn search_pipe(&self, pipe_name: &str, query_vector: &[f32], top_k: usize) -> Vec<String> {
        if let Some(pipe) = self.memory_pipes.get(pipe_name) {
            let mut scored_chunks: Vec<(f32, String)> = pipe.iter().map(|chunk| {
                let score = Self::cosine_similarity(&chunk.vector, query_vector);
                (score, chunk.text.clone())
            }).collect();

            // Sort descending by highest match score
            scored_chunks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            return scored_chunks.into_iter().take(top_k).map(|(_, text)| text).collect();
        }
        vec![]
    }

}

pub struct RateLimiter {
    usage: DashMap<String, (u32, Instant)>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            usage: DashMap::new(),
        }
    }

    /// checks if the Agent has exceeded its allowed quota per minute
    pub fn check_and_add(&self, app_id: &str, limit: u32, requested_tokens: u32) -> bool {
        let mut entry = self.usage.entry(app_id.to_string()).or_insert((0, Instant::now()));
        
        // reset the counter if a minute has passed
        if entry.1.elapsed() > Duration::from_secs(60) {
            entry.0 = 0;
            entry.1 = Instant::now();
        }

        if entry.0 + requested_tokens > limit {
            return false; 
        }

        entry.0 += requested_tokens;
        true
    }
}