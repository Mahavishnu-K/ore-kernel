use dashmap::DashMap;
use tokio::sync::broadcast;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

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
    pub source_app: String,
    pub timestamp: u64,
}

pub struct SemanticBus {
    // pipe_name -> list of memory chunks
    memory_pipes: DashMap<String, Vec<MemoryChunk>>,

    // cache maps hash(text) -> vector. Prevents wasting CPU on identical text
    embedding_cache: DashMap<u64, Vec<f32>>,
}

impl SemanticBus {
    pub fn new() -> Self {
        Self {
            memory_pipes: DashMap::new(),
            embedding_cache: DashMap::new(),
        }
    }

    pub fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    pub fn get_cached_embedding(&self, text: &str) -> Option<Vec<f32>> {
        let hash = Self::hash_text(text);
        self.embedding_cache.get(&hash).map(|v| v.clone())
    }

    pub fn write_chunk(&self, pipe_name: &str, text: String, vector: Vec<f32>,  source_app: &str) {
        let hash = Self::hash_text(&text);
        self.embedding_cache.insert(hash, vector.clone());

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

        let mut pipe = self.memory_pipes.entry(pipe_name.to_string()).or_insert_with(Vec::new);
        pipe.push(MemoryChunk { 
            text, 
            vector, 
            source_app: source_app.to_string(),
            timestamp 
        });
    }

    /// Core Vector Search Algorithm (Cosine Similarity)
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
    }

    /// Searches the pipe and returns the top 3 most relevant text chunks based on cosine similarity
    pub fn search_pipe(&self, pipe_name: &str, query_vector: &[f32], top_k: usize, filter_app: Option<&str>)  -> Vec<String> {
        if let Some(pipe) = self.memory_pipes.get(pipe_name) {
            
            let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

            let mut scored_chunks: Vec<(f32, String)> = pipe.iter()
                .filter(|chunk| filter_app.map_or(true, |app| chunk.source_app == app))
                .map(|chunk| {
                    let base_score = Self::cosine_similarity(&chunk.vector, query_vector);

                    // time decay - Older memories lose slight relevance (1% drop per hour old)
                    let hours_old = (current_time.saturating_sub(chunk.timestamp)) as f32 / 3600.0;
                    let decay_factor = (1.0 - (hours_old * 0.01)).clamp(0.5, 1.0); 
                    
                    let final_score = base_score * decay_factor;

                    (final_score, chunk.text.clone())
                }).collect();

            // Sort descending by highest match score
            scored_chunks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            return scored_chunks.into_iter().take(top_k).map(|(_, text)| text).collect();
        }
        vec![]
    }

    pub fn create_sliding_windows(text: &str, window_size: usize, overlap: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut chunks = Vec::new();
        let mut i = 0;

        // Slide forward, but keep 'overlap' words from the previous chunk to maintain context
        while i < words.len() {
            let end = std::cmp::min(i + window_size, words.len());
            chunks.push(words[i..end].join(" "));
            if end == words.len() { break; }
            i += window_size - overlap; 
        }
        chunks
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