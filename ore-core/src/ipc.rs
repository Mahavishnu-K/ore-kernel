use dashmap::DashMap;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::vec;
use tokio::sync::mpsc;

// Inter-process communication structures and utilities for ORE Agents
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentMessage {
    pub from_app: String,
    pub to_app: String,
    pub payload: String,
    pub timestamp: u64,
}

pub struct MessageBus {
    channel: DashMap<
        String,
        (
            mpsc::UnboundedSender<AgentMessage>,
            StdMutex<mpsc::UnboundedReceiver<AgentMessage>>,
        ),
    >,
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            channel: DashMap::new(),
        }
    }

    /// App A sends a message to App B
    pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
        let target_channel = self.channel.entry(msg.to_app.clone()).or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            (tx, StdMutex::new(rx))
        });

        target_channel
            .0
            .send(msg)
            .map_err(|_| "Failed to deliver message.".to_string())
    }

    /// App B registers itself to listen for messages
    pub fn read_message(&self, app_id: &str) -> Option<AgentMessage> {
        if let Some(target_channel) = self.channel.get(app_id) {
            let mut rx = target_channel.1.lock().unwrap();
            // Non-blocking read
            if let Ok(msg) = rx.try_recv() {
                return Some(msg);
            }
        }
        None
    }
}

// In-memory shared data pipes for semantic communication pipe
// Tier 2: The lazy semantic bus (System-Level Vector DB)
#[derive(Clone, Debug)]
pub struct MemoryChunk {
    pub text: Arc<String>,
    pub vector: Arc<Vec<f32>>, // holds a PRE-NORMALIZED vector
    pub source_app: String,
    pub timestamp: u64,
}

/// Helper struct for the Top-K BinaryHeap
struct ScoredChunk {
    score: f32,
    chunk: Arc<MemoryChunk>,
}

impl PartialEq for ScoredChunk {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for ScoredChunk {}

impl Ord for ScoredChunk {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for ScoredChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct SemanticBus {
    memory_pipes: DashMap<String, VecDeque<Arc<MemoryChunk>>>,
    embedding_cache: DashMap<u64, (Arc<Vec<f32>>, u64)>,
    cache_ttl_secs: u64,
    pipe_ttl_secs: u64,
}

impl SemanticBus {
    pub fn new(cache_ttl_hours: u64, pipe_ttl_hours: u64) -> Self {
        Self {
            memory_pipes: DashMap::new(),
            embedding_cache: DashMap::new(),
            cache_ttl_secs: cache_ttl_hours * 3600,
            pipe_ttl_secs: pipe_ttl_hours * 3600,
        }
    }

    /// ORE Internal: Caches a mathematical vector without exposing it to any pipes.
    pub fn cache_only(&self, text: &str, vector: Arc<Vec<f32>>) {
        let hash = Self::hash_text(text);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // This stays in the internal system cache, hidden from 'ore ls' and other pipes.
        self.embedding_cache.insert(hash, (vector, timestamp));
    }

    pub fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    pub fn get_cached_embedding(&self, text: &str) -> Option<Arc<Vec<f32>>> {
        let hash = Self::hash_text(text);
        if let Some(entry) = self.embedding_cache.get(&hash) {
            let (vector, timestamp) = entry.value();

            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            // Dynamic TTL Revalidation!
            // Only return it if cache_ttl_secs is 0 (infinite) OR it is younger than the TTL.
            if self.cache_ttl_secs == 0
                || current_time.saturating_sub(*timestamp) < self.cache_ttl_secs
            {
                return Some(vector.clone());
            }
        }
        None
    }

    fn normalize(vec: &mut [f32]) {
        let sum_sq: f32 = vec.iter().map(|x| x * x).sum();
        let norm = sum_sq.sqrt().max(1e-9);
        for x in vec.iter_mut() {
            *x /= norm;
        }
    }

    pub fn write_chunk(
        &self,
        pipe_name: &str,
        text: String,
        mut vector: Vec<f32>,
        source_app: &str,
    ) {
        let hash = Self::hash_text(&text);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self::normalize(&mut vector);

        let arc_vector = Arc::new(vector);
        let arc_text = Arc::new(text);
        let arc_chunk = Arc::new(MemoryChunk {
            text: arc_text,
            vector: Arc::clone(&arc_vector),
            source_app: source_app.to_string(),
            timestamp,
        });

        self.embedding_cache
            .insert(hash, (Arc::clone(&arc_vector), timestamp));

        let mut pipe = self.memory_pipes.entry(pipe_name.to_string()).or_default();
        pipe.push_back(arc_chunk);

        if pipe.len() > 10_000 {
            pipe.remove(0);
        }
    }

    /// Optimized Similarity: Since vectors are normalized, Cosine Similarity is just a Dot Product!
    fn dot_product(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Searches the pipe and returns the top 3 most relevant text chunks
    pub fn search_pipe(
        &self,
        pipe_name: &str,
        query_vector: &[f32],
        top_k: usize,
        filter_app: Option<&str>,
    ) -> Vec<(f32, Arc<MemoryChunk>)> {
        if let Some(pipe) = self.memory_pipes.get(pipe_name) {
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let mut heap = BinaryHeap::with_capacity(top_k);

            for chunk in pipe.iter() {
                if let Some(app_id) = filter_app
                    && chunk.source_app != app_id
                {
                    continue;
                }

                // 2. DOT PRODUCT (Fast math)
                let base_score = Self::dot_product(&chunk.vector, query_vector);

                // 3. TIME DECAY
                let hours_old = (current_time.saturating_sub(chunk.timestamp)) as f32 / 3600.0;
                let decay_factor = (1.0 - (hours_old * 0.01)).clamp(0.5, 1.0);
                let final_score = base_score * decay_factor;

                // 4. HEAP INSERTION (O(log K) instead of O(log N))
                if heap.len() < top_k {
                    heap.push(ScoredChunk {
                        score: final_score,
                        chunk: Arc::clone(chunk),
                    });
                } else if let Some(mut root) = heap.peek_mut()
                    && final_score > root.score
                {
                    *root = ScoredChunk {
                        score: final_score,
                        chunk: Arc::clone(chunk),
                    };
                }
            }

            // Convert heap to sorted vector
            let mut results: Vec<_> = heap.into_iter().map(|s| (s.score, s.chunk)).collect();
            results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            return results;
        }
        vec![]
    }

    fn create_sliding_windows(text: &str, window_size: usize, overlap: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut chunks = Vec::new();
        let mut i = 0;

        let step = std::cmp::max(1, window_size.saturating_sub(overlap));

        while i < words.len() {
            let end = std::cmp::min(i + window_size, words.len());
            chunks.push(words[i..end].join(" "));
            if end == words.len() {
                break;
            }
            i += step;
        }
        chunks
    }

    pub fn chunk_text(text: &str, strategy: &str, size: usize, overlap: usize) -> Vec<String> {
        match strategy {
            "sliding_window" => Self::create_sliding_windows(text, size, overlap),

            "sentence_aware" => {
                // Future Implementation: Use a regex to split by [. ! ?]
                // For now, fallback to sliding window to prevent crashes.
                println!(
                    "-> [KERNEL WARN] SentenceAware not yet implemented. Falling back to SlidingWindow."
                );
                Self::create_sliding_windows(text, size, overlap)
            }

            "paragraph" => {
                // Future Implementation: Split by \n\n
                println!(
                    "-> [KERNEL WARN] Paragraph not yet implemented. Falling back to SlidingWindow."
                );
                Self::create_sliding_windows(text, size, overlap)
            }

            "exact_match" => {
                // Don't chunk at all! (Used for injecting exact code blocks or API keys)
                vec![text.to_string()]
            }

            _ => Self::create_sliding_windows(text, size, overlap),
        }
    }

    /// Garbage Collector wipes cached math and unused temporary pipes
    pub fn run_garbage_collection(&self) {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if self.cache_ttl_secs > 0 {
            let initial_cache_size = self.embedding_cache.len();

            self.embedding_cache.retain(|_, (_, timestamp)| {
                current_time.saturating_sub(*timestamp) < self.cache_ttl_secs
            });

            let cleaned_cache = initial_cache_size - self.embedding_cache.len();
            if cleaned_cache > 0 {
                println!(
                    "-> [KERNEL GC] Swept {} stale embedding calculations from RAM.",
                    cleaned_cache
                );
            }
        }

        if self.pipe_ttl_secs > 0 {
            let mut pipes_cleaned = 0;
            let mut chunks_swept = 0;

            for mut pipe_ref in self.memory_pipes.iter_mut() {
                let pipe_contents = pipe_ref.value_mut();
                let initial_chunks = pipe_contents.len();

                pipe_contents.retain(|chunk| {
                    current_time.saturating_sub(chunk.timestamp) < self.pipe_ttl_secs
                });

                let removed = initial_chunks - pipe_contents.len();
                if removed > 0 {
                    chunks_swept += removed;
                    pipes_cleaned += 1;
                }
            }

            self.memory_pipes.retain(|_, chunks| !chunks.is_empty());

            if chunks_swept > 0 {
                println!(
                    "-> [KERNEL GC] Swept {} stale memory chunks across {} Semantic Pipes.",
                    chunks_swept, pipes_cleaned
                );
            }
        }
    }
}

pub struct RateLimiter {
    usage: DashMap<String, (u32, Instant)>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            usage: DashMap::new(),
        }
    }

    /// checks if the Agent has exceeded its allowed quota per minute
    pub fn check_and_add(&self, app_id: &str, limit: u32, requested_tokens: u32) -> bool {
        let mut entry = self
            .usage
            .entry(app_id.to_string())
            .or_insert((0, Instant::now()));

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
