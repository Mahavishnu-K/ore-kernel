use dashmap::DashMap;
use ore_common::protocol::MemoryCategory;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

// ─── TIER 1: BOUNDED MESSAGE BUS ─────────────────────────────────────────────

pub const DEFAULT_MESSAGE_BACKLOG: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
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
            mpsc::Sender<AgentMessage>,
            StdMutex<mpsc::Receiver<AgentMessage>>,
        ),
    >,
    backlog_capacity: usize,
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
            backlog_capacity: DEFAULT_MESSAGE_BACKLOG,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            channel: DashMap::new(),
            backlog_capacity: capacity.max(1),
        }
    }

    /// App A sends a message to App B with bounded queue protection (Prevents OOM)
    pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
        let target_channel = self.channel.entry(msg.to_app.clone()).or_insert_with(|| {
            let (tx, rx) = mpsc::channel(self.backlog_capacity);
            (tx, StdMutex::new(rx))
        });

        target_channel.0.try_send(msg).map_err(|e| match e {
            mpsc::error::TrySendError::Full(dropped) => {
                format!(
                    "Target agent '{}' queue is full (capacity: {}). Backpressure applied.",
                    dropped.to_app, self.backlog_capacity
                )
            }
            mpsc::error::TrySendError::Closed(_) => {
                "Target agent channel receiver has closed.".to_string()
            }
        })
    }

    /// App B reads next pending message non-blockingly
    pub fn read_message(&self, app_id: &str) -> Option<AgentMessage> {
        if let Some(target_channel) = self.channel.get(app_id) {
            let mut rx = target_channel.1.lock().unwrap();
            if let Ok(msg) = rx.try_recv() {
                return Some(msg);
            }
        }
        None
    }
}

// ─── TIER 2: HIGH-PERFORMANCE BM25 SPARSE INDEX ───────────────────────────────

/// Fast in-memory tokenizer designed for code, hex addresses, identifiers, and natural text.
pub fn tokenize_bm25(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// In-memory BM25 Inverted Index for exact keyword matching (identifiers, error codes, symbols)
#[derive(Clone, Debug, Default)]
pub struct Bm25Index {
    /// term -> list of (chunk_id, term_frequency)
    pub inverted_index: HashMap<String, Vec<(u64, u32)>>,
    /// chunk_id -> document token count
    pub doc_lengths: HashMap<u64, usize>,
    /// Total tokens across all indexed chunks in this pipe
    pub total_tokens: usize,
}

impl Bm25Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_chunks(chunks: &VecDeque<Arc<MemoryChunk>>) -> Self {
        let mut idx = Self::new();
        for chunk in chunks {
            idx.add_chunk(chunk.id, &chunk.text);
        }
        idx
    }

    pub fn add_chunk(&mut self, chunk_id: u64, text: &str) {
        let tokens = tokenize_bm25(text);
        let doc_len = tokens.len();

        let mut freqs: HashMap<String, u32> = HashMap::new();
        for token in tokens {
            *freqs.entry(token).or_insert(0) += 1;
        }

        for (term, freq) in freqs {
            self.inverted_index
                .entry(term)
                .or_default()
                .push((chunk_id, freq));
        }

        self.doc_lengths.insert(chunk_id, doc_len);
        self.total_tokens += doc_len;
    }

    pub fn score_query(&self, query_text: &str) -> HashMap<u64, f32> {
        let tokens = tokenize_bm25(query_text);
        let mut scores: HashMap<u64, f32> = HashMap::new();

        let n_docs = self.doc_lengths.len();
        if n_docs == 0 {
            return scores;
        }

        let avg_dl = (self.total_tokens as f32 / n_docs as f32).max(1e-5);
        let k1: f32 = 1.2;
        let b: f32 = 0.75;

        let unique_terms: HashSet<_> = tokens.into_iter().collect();

        for term in unique_terms {
            if let Some(postings) = self.inverted_index.get(&term) {
                let n_t = postings.len() as f32;
                // Standard BM25 Robertson-Spärck Jones IDF with smoothing
                let idf = (((n_docs as f32 - n_t + 0.5) / (n_t + 0.5)) + 1.0)
                    .ln()
                    .max(0.0);

                for &(chunk_id, tf) in postings {
                    let doc_len = *self.doc_lengths.get(&chunk_id).unwrap_or(&1) as f32;
                    let tf_component = (tf as f32 * (k1 + 1.0))
                        / (tf as f32 + k1 * (1.0 - b + b * (doc_len / avg_dl)));
                    *scores.entry(chunk_id).or_insert(0.0) += idf * tf_component;
                }
            }
        }

        scores
    }
}

// ─── TIER 2: MEMORY CHUNK & COGNITIVE MMU PIPES ───────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: u64,
    pub text: Arc<String>,
    pub vector: Arc<Vec<f32>>, // Pre-normalized unit vector
    pub source_app: String,
    pub timestamp: u64,
    pub category: MemoryCategory,
}

/// Explicit lifecycle state for Semantic Pipes.
/// Eliminates the GC ghost-pipe bug where paged pipes were permanently silenced.
#[derive(Clone, Debug)]
pub enum PipeState {
    Active {
        is_persistent: bool,
        chunks: VecDeque<Arc<MemoryChunk>>,
        bm25: Bm25Index,
    },
    PagedToDisk,
}

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
        // Min-heap behavior so we can pop the smallest score when capacity is reached
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
    memory_pipes: DashMap<String, PipeState>,
    embedding_cache: DashMap<u64, (Arc<Vec<f32>>, u64)>,
    cache_ttl_secs: u64,
    pipe_ttl_secs: u64,
    next_chunk_id: AtomicU64,
}

impl SemanticBus {
    pub fn new(cache_ttl_hours: u64, pipe_ttl_hours: u64) -> Self {
        Self {
            memory_pipes: DashMap::new(),
            embedding_cache: DashMap::new(),
            cache_ttl_secs: cache_ttl_hours * 3600,
            pipe_ttl_secs: pipe_ttl_hours * 3600,
            next_chunk_id: AtomicU64::new(1),
        }
    }

    pub fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    pub fn cache_only(&self, text: &str, vector: Arc<Vec<f32>>) {
        let hash = Self::hash_text(text);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.embedding_cache.insert(hash, (vector, timestamp));
    }

    pub fn get_cached_embedding(&self, text: &str) -> Option<Arc<Vec<f32>>> {
        let hash = Self::hash_text(text);
        if let Some(entry) = self.embedding_cache.get(&hash) {
            let (vector, timestamp) = entry.value();
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if self.cache_ttl_secs == 0
                || current_time.saturating_sub(*timestamp) < self.cache_ttl_secs
            {
                return Some(vector.clone());
            }
        }
        None
    }

    /// L2 Normalization in-place
    fn normalize(vec: &mut [f32]) {
        let sum_sq: f32 = vec.iter().map(|x| x * x).sum();
        let norm = sum_sq.sqrt().max(1e-9);
        for x in vec.iter_mut() {
            *x /= norm;
        }
    }

    /// High-performance 8-lane loop unrolled dot product.
    /// Breaks instruction dependency chains and auto vectorizes via AVX2/FMA/NEON.
    #[inline(always)]
    pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let mut sum0 = 0.0f32;
        let mut sum1 = 0.0f32;
        let mut sum2 = 0.0f32;
        let mut sum3 = 0.0f32;
        let mut sum4 = 0.0f32;
        let mut sum5 = 0.0f32;
        let mut sum6 = 0.0f32;
        let mut sum7 = 0.0f32;

        let chunks_a = a.chunks_exact(8);
        let chunks_b = b.chunks_exact(8);
        let rem_a = chunks_a.remainder();
        let rem_b = chunks_b.remainder();

        for (ca, cb) in chunks_a.zip(chunks_b) {
            sum0 += ca[0] * cb[0];
            sum1 += ca[1] * cb[1];
            sum2 += ca[2] * cb[2];
            sum3 += ca[3] * cb[3];
            sum4 += ca[4] * cb[4];
            sum5 += ca[5] * cb[5];
            sum6 += ca[6] * cb[6];
            sum7 += ca[7] * cb[7];
        }

        let mut total = (sum0 + sum1) + (sum2 + sum3) + (sum4 + sum5) + (sum6 + sum7);
        for (ra, rb) in rem_a.iter().zip(rem_b.iter()) {
            total += ra * rb;
        }
        total
    }

    pub fn write_chunk(
        &self,
        pipe_name: &str,
        text: String,
        mut vector: Vec<f32>,
        source_app: &str,
        is_persistent: bool,
        category: MemoryCategory,
    ) {
        Self::normalize(&mut vector);
        let arc_vector = Arc::new(vector);
        let arc_text = Arc::new(text);

        let hash = Self::hash_text(&arc_text);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let chunk_id = self.next_chunk_id.fetch_add(1, AtomicOrdering::SeqCst);

        let arc_chunk = Arc::new(MemoryChunk {
            id: chunk_id,
            text: arc_text,
            vector: Arc::clone(&arc_vector),
            source_app: source_app.to_string(),
            timestamp,
            category,
        });

        self.embedding_cache
            .insert(hash, (Arc::clone(&arc_vector), timestamp));

        self.insert_chunk_to_pipe(pipe_name, arc_chunk, is_persistent);
    }

    pub fn write_cached_chunk(
        &self,
        pipe_name: &str,
        text: String,
        arc_vector: Arc<Vec<f32>>,
        source_app: &str,
        is_persistent: bool,
        category: MemoryCategory,
    ) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let chunk_id = self.next_chunk_id.fetch_add(1, AtomicOrdering::SeqCst);

        let arc_chunk = Arc::new(MemoryChunk {
            id: chunk_id,
            text: Arc::new(text),
            vector: arc_vector,
            source_app: source_app.to_string(),
            timestamp,
            category,
        });

        self.insert_chunk_to_pipe(pipe_name, arc_chunk, is_persistent);
    }

    fn insert_chunk_to_pipe(&self, pipe_name: &str, chunk: Arc<MemoryChunk>, is_persistent: bool) {
        let mut entry = self
            .memory_pipes
            .entry(pipe_name.to_string())
            .or_insert_with(|| PipeState::Active {
                is_persistent,
                chunks: VecDeque::new(),
                bm25: Bm25Index::new(),
            });

        match entry.value_mut() {
            PipeState::Active {
                is_persistent: p,
                chunks,
                bm25,
            } => {
                if is_persistent {
                    *p = true;
                }
                bm25.add_chunk(chunk.id, &chunk.text);
                chunks.push_back(chunk);
                if chunks.len() > 10_000 {
                    chunks.pop_front();
                }
            }
            state @ PipeState::PagedToDisk => {
                // Page fault recovery: restore existing chunks from SSD before appending
                let mut restored =
                    crate::memory::Pager::page_in_semantic(pipe_name).unwrap_or_default();

                let max_id = restored.iter().map(|c| c.id).max().unwrap_or(0);
                self.next_chunk_id
                    .fetch_max(max_id + 1, AtomicOrdering::SeqCst);

                let mut bm25 = Bm25Index::from_chunks(&restored);
                bm25.add_chunk(chunk.id, &chunk.text);

                restored.push_back(chunk);
                if restored.len() > 10_000 {
                    restored.pop_front();
                }
                *state = PipeState::Active {
                    is_persistent: true,
                    chunks: restored,
                    bm25,
                };
            }
        }
    }

    pub fn get_pipe_contents(&self, pipe_name: &str) -> Option<VecDeque<Arc<MemoryChunk>>> {
        let needs_load = match self.memory_pipes.get(pipe_name) {
            None => true,
            Some(entry) => matches!(entry.value(), PipeState::PagedToDisk),
        };

        if needs_load && let Some(restored) = crate::memory::Pager::page_in_semantic(pipe_name) {
            let max_id = restored.iter().map(|c| c.id).max().unwrap_or(0);
            self.next_chunk_id
                .fetch_max(max_id + 1, AtomicOrdering::SeqCst);
            let bm25 = Bm25Index::from_chunks(&restored);

            self.memory_pipes.insert(
                pipe_name.to_string(),
                PipeState::Active {
                    is_persistent: true,
                    chunks: restored,
                    bm25,
                },
            );
        }

        self.memory_pipes
            .get(pipe_name)
            .and_then(|pipe| match pipe.value() {
                PipeState::Active { chunks, .. } => Some(chunks.clone()),
                PipeState::PagedToDisk => None,
            })
    }

    /// Dual-Pass Hybrid Search (BM25 Sparse + Dense SIMD fused via Reciprocal Rank Fusion)
    #[allow(clippy::too_many_arguments)]
    pub fn search_pipe(
        &self,
        pipe_name: &str,
        query_vector: &[f32],
        query_text: &str,
        top_k: usize,
        filter_app: Option<&str>,
        filter_category: Option<MemoryCategory>,
        allow_time_decay: bool,
        enable_hybrid: bool,
    ) -> Vec<(f32, Arc<MemoryChunk>)> {
        if top_k == 0 {
            return vec![];
        }

        // PAGE FAULT RECOVERY: Verify if pipe is unloaded or in PagedToDisk state
        let needs_load = match self.memory_pipes.get(pipe_name) {
            None => true,
            Some(entry) => matches!(entry.value(), PipeState::PagedToDisk),
        };

        if needs_load {
            if let Some(restored) = crate::memory::Pager::page_in_semantic(pipe_name) {
                let max_id = restored.iter().map(|c| c.id).max().unwrap_or(0);
                self.next_chunk_id
                    .fetch_max(max_id + 1, AtomicOrdering::SeqCst);
                let bm25 = Bm25Index::from_chunks(&restored);

                self.memory_pipes.insert(
                    pipe_name.to_string(),
                    PipeState::Active {
                        is_persistent: true,
                        chunks: restored,
                        bm25,
                    },
                );
            } else if !self.memory_pipes.contains_key(pipe_name) {
                return vec![];
            }
        }

        let pipe_guard = match self.memory_pipes.get(pipe_name) {
            Some(g) => g,
            None => return vec![],
        };

        let (chunks, bm25) = match pipe_guard.value() {
            PipeState::Active { chunks, bm25, .. } => (chunks, bm25),
            PipeState::PagedToDisk => return vec![],
        };

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // ── PASS 1: Dense SIMD Scoring with BinaryHeap ─────────────────────────
        // For hybrid search, retain a candidate pool of (top_k * 4). For dense-only, retain top_k.
        let dense_candidate_limit = if enable_hybrid {
            (top_k * 4).clamp(32, 200)
        } else {
            top_k
        };

        let mut dense_heap = BinaryHeap::with_capacity(dense_candidate_limit);

        for chunk in chunks.iter() {
            if let Some(app_id) = filter_app
                && chunk.source_app != app_id
            {
                continue;
            }

            if let Some(cat) = filter_category
                && chunk.category != cat
            {
                continue;
            }

            // SIMD Dot Product
            let base_score = Self::dot_product(&chunk.vector, query_vector);
            let mut final_score = base_score;

            if allow_time_decay {
                let hours_old = (current_time.saturating_sub(chunk.timestamp)) as f32 / 3600.0;
                let decay_factor = (1.0 - (hours_old * 0.01)).clamp(0.5, 1.0);
                final_score *= decay_factor;
            }

            if dense_heap.len() < dense_candidate_limit {
                dense_heap.push(ScoredChunk {
                    score: final_score,
                    chunk: Arc::clone(chunk),
                });
            } else if let Some(mut root) = dense_heap.peek_mut()
                && final_score > root.score
            {
                *root = ScoredChunk {
                    score: final_score,
                    chunk: Arc::clone(chunk),
                };
            }
        }

        // If hybrid search is disabled, return directly from the heap
        if !enable_hybrid {
            let mut results: Vec<_> = dense_heap.into_iter().map(|s| (s.score, s.chunk)).collect();
            results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
            return results;
        }

        // Explicit descending sort by score (highest score at index 0)
        let mut dense_ranked: Vec<ScoredChunk> = dense_heap.into_vec();
        dense_ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        // ── PASS 2: BM25 Sparse Scoring with BinaryHeap ────────────────────────
        let bm25_scores = bm25.score_query(query_text);
        let sparse_candidate_limit = (top_k * 4).clamp(32, 200);
        let mut sparse_heap = BinaryHeap::with_capacity(sparse_candidate_limit);

        for chunk in chunks.iter() {
            if let Some(app_id) = filter_app
                && chunk.source_app != app_id
            {
                continue;
            }
            if let Some(cat) = filter_category
                && chunk.category != cat
            {
                continue;
            }

            if let Some(&score) = bm25_scores.get(&chunk.id)
                && score > 0.0
            {
                if sparse_heap.len() < sparse_candidate_limit {
                    sparse_heap.push(ScoredChunk {
                        score,
                        chunk: Arc::clone(chunk),
                    });
                } else if let Some(mut root) = sparse_heap.peek_mut()
                    && score > root.score
                {
                    *root = ScoredChunk {
                        score,
                        chunk: Arc::clone(chunk),
                    };
                }
            }
        }

        // Explicit descending sort by score (highest score at index 0)
        let mut sparse_ranked: Vec<ScoredChunk> = sparse_heap.into_vec();
        sparse_ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        // ── PASS 3: Reciprocal Rank Fusion (RRF) ────────────────────────────────
        const RRF_K: f32 = 60.0;
        let mut rrf_scores: HashMap<u64, (f32, Arc<MemoryChunk>)> = HashMap::new();

        // Highest score gets rank 1 (rank_idx = 0 -> rank = 1.0)
        for (rank_idx, scored) in dense_ranked.into_iter().enumerate() {
            let rank = (rank_idx + 1) as f32;
            let entry = rrf_scores
                .entry(scored.chunk.id)
                .or_insert((0.0, scored.chunk));
            entry.0 += 1.0 / (RRF_K + rank);
        }

        for (rank_idx, scored) in sparse_ranked.into_iter().enumerate() {
            let rank = (rank_idx + 1) as f32;
            let entry = rrf_scores
                .entry(scored.chunk.id)
                .or_insert((0.0, scored.chunk));
            entry.0 += 1.0 / (RRF_K + rank);
        }

        // Final Top-K selection using BinaryHeap
        let mut final_heap = BinaryHeap::with_capacity(top_k);

        for (_, (score, chunk)) in rrf_scores {
            if final_heap.len() < top_k {
                final_heap.push(ScoredChunk { score, chunk });
            } else if let Some(mut root) = final_heap.peek_mut()
                && score > root.score
            {
                *root = ScoredChunk { score, chunk };
            }
        }

        let mut results: Vec<_> = final_heap.into_iter().map(|s| (s.score, s.chunk)).collect();
        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        results
    }

    // ─── CHUNKING ENGINE ─────────────────────────────────────────────────────────

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

    /// Splits text into natural sentence units and aggregates them up to window_size words
    fn create_sentence_aware_chunks(
        text: &str,
        target_words: usize,
        overlap_words: usize,
    ) -> Vec<String> {
        let mut sentences = Vec::new();
        let mut current_sentence = String::new();
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            current_sentence.push(c);
            if matches!(c, '.' | '!' | '?')
                && let Some(&next) = chars.peek()
                && next.is_whitespace()
            {
                let trimmed = current_sentence.trim();
                if !trimmed.is_empty() {
                    sentences.push(trimmed.to_string());
                }
                current_sentence.clear();
            }
        }

        let tail = current_sentence.trim();
        if !tail.is_empty() {
            sentences.push(tail.to_string());
        }

        if sentences.is_empty() {
            return Self::create_sliding_windows(text, target_words, overlap_words);
        }

        let mut chunks = Vec::new();
        let mut current_chunk_sentences: Vec<String> = Vec::new();
        let mut current_word_count = 0;

        for sentence in sentences {
            let s_words = sentence.split_whitespace().count();

            // If a single sentence exceeds target words, fallback to sliding window for that sentence
            if s_words >= target_words {
                if !current_chunk_sentences.is_empty() {
                    chunks.push(current_chunk_sentences.join(" "));
                    current_chunk_sentences.clear();
                    current_word_count = 0;
                }
                chunks.extend(Self::create_sliding_windows(
                    &sentence,
                    target_words,
                    overlap_words,
                ));
                continue;
            }

            if current_word_count + s_words > target_words && !current_chunk_sentences.is_empty() {
                chunks.push(current_chunk_sentences.join(" "));

                // Keep overlapping tail sentences
                let mut kept_sentences = Vec::new();
                let mut kept_words = 0;
                for s in current_chunk_sentences.iter().rev() {
                    let w = s.split_whitespace().count();
                    if kept_words + w <= overlap_words {
                        kept_sentences.insert(0, s.clone());
                        kept_words += w;
                    } else {
                        break;
                    }
                }
                current_chunk_sentences = kept_sentences;
                current_word_count = kept_words;
            }

            current_word_count += s_words;
            current_chunk_sentences.push(sentence);
        }

        if !current_chunk_sentences.is_empty() {
            chunks.push(current_chunk_sentences.join(" "));
        }

        chunks
    }

    /// Splits by double-newlines (\n\n) preserving whole thoughts and paragraphs
    fn create_paragraph_chunks(text: &str, max_words: usize) -> Vec<String> {
        let paragraphs: Vec<&str> = text
            .split("\n\n")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();

        if paragraphs.is_empty() {
            return Self::create_sliding_windows(text, max_words, max_words / 5);
        }

        let mut chunks = Vec::new();
        let mut current_buffer = Vec::new();
        let mut current_count = 0;

        for p in paragraphs {
            let p_words = p.split_whitespace().count();

            if p_words > max_words {
                if !current_buffer.is_empty() {
                    chunks.push(current_buffer.join("\n\n"));
                    current_buffer.clear();
                    current_count = 0;
                }
                chunks.extend(Self::create_sliding_windows(p, max_words, max_words / 5));
                continue;
            }

            if current_count + p_words > max_words && !current_buffer.is_empty() {
                chunks.push(current_buffer.join("\n\n"));
                current_buffer.clear();
                current_count = 0;
            }

            current_count += p_words;
            current_buffer.push(p);
        }

        if !current_buffer.is_empty() {
            chunks.push(current_buffer.join("\n\n"));
        }

        chunks
    }

    pub fn chunk_text(text: &str, strategy: &str, size: usize, overlap: usize) -> Vec<String> {
        match strategy {
            "sliding_window" => Self::create_sliding_windows(text, size, overlap),
            "sentence_aware" => Self::create_sentence_aware_chunks(text, size, overlap),
            "paragraph" => Self::create_paragraph_chunks(text, size),
            "exact_match" => vec![text.to_string()],
            _ => Self::create_sliding_windows(text, size, overlap),
        }
    }

    /// Routine background garbage collection.
    /// Safely syncs persistent pipes to SSD and sets them to PagedToDisk rather than losing data.
    pub fn run_garbage_collection(&self) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if self.cache_ttl_secs > 0 {
            let initial_cache_size = self.embedding_cache.len();
            self.embedding_cache.retain(|_, (_, timestamp)| {
                current_time.saturating_sub(*timestamp) < self.cache_ttl_secs
            });
            let cleaned = initial_cache_size - self.embedding_cache.len();
            if cleaned > 0 {
                crate::kprintln!(
                    "-> [KERNEL GC] Swept {} stale embedding calculations from RAM.",
                    cleaned
                );
            }
        }

        if self.pipe_ttl_secs > 0 {
            let mut pipes_evicted = 0;
            let mut chunks_swept = 0;

            for mut pipe_ref in self.memory_pipes.iter_mut() {
                let pipe_name = pipe_ref.key().clone();
                let mut evict_to_disk = false;

                match pipe_ref.value_mut() {
                    PipeState::Active {
                        is_persistent,
                        chunks,
                        ..
                    } => {
                        if *is_persistent {
                            if !chunks.is_empty() {
                                crate::memory::Pager::page_out_semantic(&pipe_name, chunks);
                                pipes_evicted += 1;
                            }
                            evict_to_disk = true;
                        } else {
                            let initial = chunks.len();
                            chunks.retain(|chunk| {
                                current_time.saturating_sub(chunk.timestamp) < self.pipe_ttl_secs
                            });
                            let removed = initial - chunks.len();
                            if removed > 0 {
                                chunks_swept += removed;
                            }
                        }
                    }
                    PipeState::PagedToDisk => {}
                }
                if evict_to_disk {
                    *pipe_ref.value_mut() = PipeState::PagedToDisk;
                }
            }

            // Retain active non-empty ephemeral pipes and all persistent pipes
            self.memory_pipes.retain(|_, state| match state {
                PipeState::Active {
                    is_persistent,
                    chunks,
                    ..
                } => *is_persistent || !chunks.is_empty(),
                PipeState::PagedToDisk => true,
            });

            if pipes_evicted > 0 || chunks_swept > 0 {
                crate::kprintln!(
                    "-> [KERNEL GC] Evicted {} persistent pipes to SSD (PagedToDisk). Swept {} stale ephemeral chunks.",
                    pipes_evicted,
                    chunks_swept
                );
            }
        }
    }

    pub fn cache_len(&self) -> usize {
        self.embedding_cache.len()
    }

    pub fn pipes_count(&self) -> usize {
        self.memory_pipes.len()
    }
}

// ─── SLIDING WINDOW RATE LIMITER ─────────────────────────────────────────────

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

    pub fn check_and_add(&self, app_id: &str, limit: u32, requested_tokens: u32) -> bool {
        let mut entry = self
            .usage
            .entry(app_id.to_string())
            .or_insert((0, Instant::now()));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_dot_product_unrolled() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let b = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];

        let expected: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let actual = SemanticBus::dot_product(&a, &b);

        assert!((expected - actual).abs() < 1e-6);
    }

    #[test]
    fn test_bounded_message_bus_backpressure() {
        let bus = MessageBus::with_capacity(2);

        let msg = |i: usize| AgentMessage {
            from_app: "sender".to_string(),
            to_app: "receiver".to_string(),
            payload: format!("msg_{}", i),
            timestamp: 100,
        };

        assert!(bus.send_message(msg(1)).is_ok());
        assert!(bus.send_message(msg(2)).is_ok());

        // Queue is now full (capacity 2). 3rd send must return Err with backpressure explanation
        let err = bus.send_message(msg(3));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("queue is full"));
    }

    #[test]
    fn test_sentence_aware_chunking() {
        let text = "Hello world! This is a test of the sentence chunker. It preserves boundaries properly? Absolutely.";
        let chunks = SemanticBus::chunk_text(text, "sentence_aware", 8, 2);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.contains("Hello world!")));
    }

    #[test]
    fn test_bm25_exact_keyword_matching() {
        let mut bm25 = Bm25Index::new();
        bm25.add_chunk(1, "Kernel panic: SIGSEGV memory corruption at 0xDEADBEEF");
        bm25.add_chunk(2, "Normal user login success on port 8080");

        let scores = bm25.score_query("0xDEADBEEF");
        assert!(scores.get(&1).unwrap() > &0.0);
        assert!(!scores.contains_key(&2));
    }

    #[test]
    fn test_cognitive_tier_filtering_in_search() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        bus.write_chunk(
            "test_pipe",
            "This is a verified fact".to_string(),
            vec.clone(),
            "agent_a",
            false,
            MemoryCategory::Fact,
        );

        bus.write_chunk(
            "test_pipe",
            "This is a procedural rule".to_string(),
            vec.clone(),
            "agent_a",
            false,
            MemoryCategory::Directive,
        );

        let results = bus.search_pipe(
            "test_pipe",
            &vec,
            "procedural rule",
            10,
            None,
            Some(MemoryCategory::Directive),
            false,
            true,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.category, MemoryCategory::Directive);
        assert!(results[0].1.text.contains("procedural rule"));
    }
}
