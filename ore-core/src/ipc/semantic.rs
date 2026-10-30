use super::bm25::Bm25Index;
use super::chunking::MemoryChunk;
use super::quant::{DiskVectorStore, PqChunk, PqCodebook, Sq8Chunk, Sq8Vector};
use dashmap::DashMap;
use ore_common::protocol::MemoryCategory;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Explicit lifecycle state supporting Modes A, B, C and Copy-on-Write branching
#[derive(Clone, Debug)]
pub enum PipeState {
    /// Mode A: In-RAM Full-Precision SIMD (< 10,000 chunks)
    Active {
        is_persistent: bool,
        chunks: VecDeque<Arc<MemoryChunk>>,
        bm25: Bm25Index,
    },
    /// Mode B: LEANN-Style Deep Cold Storage (> 10,000 chunks, on-the-fly candidate recompute)
    ActivePqRecompute {
        codebook: Arc<PqCodebook>,
        pq_chunks: VecDeque<Arc<PqChunk>>,
        bm25: Bm25Index,
    },
    /// Mode C: DiskANN Enterprise NVMe (> 50,000 chunks, SQ8 int8 RAM + f32 on SSD)
    ActiveSq8Nvme {
        sq8_chunks: VecDeque<Arc<Sq8Chunk>>,
        bm25: Bm25Index,
        vector_dim: usize,
    },
    // Copy-on-Write Branch
    Branched {
        base_pipe: String,
        delta_chunks: VecDeque<Arc<MemoryChunk>>,
        bm25: Bm25Index,
    },
    PagedToDisk,
}

pub struct ScoredChunk {
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

#[derive(Clone, Debug)]
pub struct SearchOutcome {
    pub results: Vec<(f32, Arc<MemoryChunk>)>,
    pub top_dense_score: f32,
}

pub struct SemanticBus {
    memory_pipes: DashMap<String, PipeState>,
    embedding_cache: DashMap<u64, (Arc<Vec<f32>>, u64)>,
    cache_ttl_secs: u64,
    pipe_ttl_secs: u64,
    next_chunk_id: AtomicU64,
}

pub struct Sq8Candidate {
    score: f32,
    f32_offset: u64,
    text: Arc<String>,
    context_payload: Arc<String>,
    source_app: String,
    timestamp: u64,
    category: MemoryCategory,
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

    #[allow(clippy::too_many_arguments)]
    pub fn write_chunk(
        &self,
        pipe_name: &str,
        text: String,
        context_payload: String,
        mut vector: Vec<f32>,
        source_app: &str,
        is_persistent: bool,
        category: MemoryCategory,
    ) {
        Self::normalize(&mut vector);
        let arc_vector = Arc::new(vector);
        let arc_text = Arc::new(text);

        // Zero-copy optimization: If context is identical to text, share the same Arc pointer
        let arc_context = if arc_text.as_str() == context_payload.as_str() {
            Arc::clone(&arc_text)
        } else {
            Arc::new(context_payload)
        };

        let hash = Self::hash_text(&arc_text);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let chunk_id = self.next_chunk_id.fetch_add(1, AtomicOrdering::SeqCst);

        let arc_chunk = Arc::new(MemoryChunk {
            id: chunk_id,
            text: arc_text,
            context_payload: arc_context,
            vector: Arc::clone(&arc_vector),
            source_app: source_app.to_string(),
            timestamp,
            category,
        });

        self.embedding_cache
            .insert(hash, (Arc::clone(&arc_vector), timestamp));

        self.insert_chunk_to_pipe(pipe_name, arc_chunk, is_persistent);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_cached_chunk(
        &self,
        pipe_name: &str,
        text: String,
        context_payload: String,
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

        let arc_text = Arc::new(text);
        let arc_context = if arc_text.as_str() == context_payload.as_str() {
            Arc::clone(&arc_text)
        } else {
            Arc::new(context_payload)
        };

        let arc_chunk = Arc::new(MemoryChunk {
            id: chunk_id,
            text: arc_text,
            context_payload: arc_context,
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
                if chunks.len() > 10_000
                    && let Some(evicted) = chunks.pop_front()
                {
                    bm25.remove_chunk(evicted.id, &evicted.text);
                }
            }
            PipeState::ActiveSq8Nvme {
                sq8_chunks,
                bm25,
                vector_dim,
            } => {
                *vector_dim = chunk.vector.len();
                let store = DiskVectorStore::new(pipe_name);
                let f32_offset = store.append(&chunk.vector).unwrap_or(0);
                let sq8_vec = Sq8Vector::quantize(&chunk.vector);

                bm25.add_chunk(chunk.id, &chunk.text);
                sq8_chunks.push_back(Arc::new(Sq8Chunk {
                    id: chunk.id,
                    text: Arc::clone(&chunk.text),
                    context_payload: Arc::clone(&chunk.context_payload),
                    sq8_vector: sq8_vec,
                    f32_offset,
                    source_app: chunk.source_app.clone(),
                    timestamp: chunk.timestamp,
                    category: chunk.category,
                }));
                if sq8_chunks.len() > 50_000 {
                    sq8_chunks.pop_front();
                }
            }
            PipeState::ActivePqRecompute {
                codebook,
                pq_chunks,
                bm25,
            } => {
                let pq_codes = codebook.encode(&chunk.vector);
                bm25.add_chunk(chunk.id, &chunk.text);
                pq_chunks.push_back(Arc::new(PqChunk {
                    id: chunk.id,
                    text: Arc::clone(&chunk.text),
                    context_payload: Arc::clone(&chunk.context_payload),
                    pq_codes,
                    source_app: chunk.source_app.clone(),
                    timestamp: chunk.timestamp,
                    category: chunk.category,
                }));
                if pq_chunks.len() > 50_000 {
                    pq_chunks.pop_front();
                }
            }
            PipeState::Branched {
                delta_chunks, bm25, ..
            } => {
                // Isolated write strictly into the branch delta! Base pipe remains untouched.
                bm25.add_chunk(chunk.id, &chunk.text);
                delta_chunks.push_back(chunk);
                if delta_chunks.len() > 10_000
                    && let Some(evicted) = delta_chunks.pop_front()
                {
                    bm25.remove_chunk(evicted.id, &evicted.text);
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

    // ─── CoW BRANCHING, DIFFING & MERGING OPERATIONS ─────────────────────────

    /// Creates an isolated Copy-on-Write memory branch from a base pipe (Zero-RAM overhead)
    pub fn branch_pipe(&self, base_pipe: &str, branch_pipe: &str) -> Result<(), String> {
        if base_pipe == branch_pipe {
            return Err("Cannot branch a pipe to itself.".to_string());
        }
        if self.memory_pipes.contains_key(branch_pipe) {
            return Err(format!("Branch pipe '{}' already exists.", branch_pipe));
        }

        // Ensure base pipe is loaded if currently paged to disk
        self.ensure_pipe_loaded(base_pipe)?;

        let base_guard = self
            .memory_pipes
            .get(base_pipe)
            .ok_or_else(|| format!("Base pipe '{}' not found.", base_pipe))?;

        let base_bm25 = match base_guard.value() {
            PipeState::Active { bm25, .. }
            | PipeState::ActivePqRecompute { bm25, .. }
            | PipeState::ActiveSq8Nvme { bm25, .. }
            | PipeState::Branched { bm25, .. } => bm25.clone(),
            PipeState::PagedToDisk => {
                return Err("Base pipe failed to page in from SSD.".to_string());
            }
        };

        drop(base_guard);

        self.memory_pipes.insert(
            branch_pipe.to_string(),
            PipeState::Branched {
                base_pipe: base_pipe.to_string(),
                delta_chunks: VecDeque::new(),
                bm25: base_bm25,
            },
        );

        crate::kprintln!(
            "-> [CoW BRANCH] Forked '{}' from '{}' (Zero-copy Arc references).",
            branch_pipe,
            base_pipe
        );

        Ok(())
    }

    /// Returns the staged delta chunks that have not yet been merged into the base pipe
    pub fn diff_pipe(
        &self,
        base_pipe: &str,
        branch_pipe: &str,
    ) -> Result<Vec<Arc<MemoryChunk>>, String> {
        let branch_guard = self
            .memory_pipes
            .get(branch_pipe)
            .ok_or_else(|| format!("Branch pipe '{}' not found.", branch_pipe))?;

        match branch_guard.value() {
            PipeState::Branched {
                base_pipe: parent,
                delta_chunks,
                ..
            } => {
                if parent != base_pipe {
                    return Err(format!(
                        "Branch '{}' is forked from '{}', not requested base '{}'.",
                        branch_pipe, parent, base_pipe
                    ));
                }
                Ok(delta_chunks.iter().cloned().collect())
            }
            _ => Err(format!("Pipe '{}' is not a branched pipe.", branch_pipe)),
        }
    }

    /// Atomically merges branch delta chunks into the base pipe and commits to SSD
    pub fn merge_pipe(
        &self,
        base_pipe: &str,
        branch_pipe: &str,
        delete_branch: bool,
    ) -> Result<usize, String> {
        let (parent_pipe, deltas) = {
            let branch_guard = self
                .memory_pipes
                .get(branch_pipe)
                .ok_or_else(|| format!("Branch pipe '{}' not found.", branch_pipe))?;

            match branch_guard.value() {
                PipeState::Branched {
                    base_pipe: parent,
                    delta_chunks,
                    ..
                } => (parent.clone(), delta_chunks.clone()),
                _ => return Err(format!("Pipe '{}' is not a branched pipe.", branch_pipe)),
            }
        };

        if parent_pipe != base_pipe {
            return Err(format!(
                "Cannot merge: Branch '{}' belongs to base '{}', not requested base '{}'.",
                branch_pipe, parent_pipe, base_pipe
            ));
        }

        let delta_count = deltas.len();
        if delta_count == 0 {
            if delete_branch {
                self.memory_pipes.remove(branch_pipe);
            }
            return Ok(0);
        }

        self.ensure_pipe_loaded(base_pipe)?;

        let mut base_entry = self
            .memory_pipes
            .get_mut(base_pipe)
            .ok_or_else(|| format!("Base pipe '{}' not found.", base_pipe))?;

        let should_persist = match base_entry.value_mut() {
            PipeState::Active {
                is_persistent,
                chunks,
                bm25,
            } => {
                for chunk in &deltas {
                    bm25.add_chunk(chunk.id, &chunk.text);
                    chunks.push_back(Arc::clone(chunk));
                    if chunks.len() > 10_000 {
                        chunks.pop_front();
                    }
                }
                *is_persistent
            }
            _ => {
                return Err(format!(
                    "Base pipe '{}' is not in an active state.",
                    base_pipe
                ));
            }
        };

        if should_persist && let PipeState::Active { chunks, .. } = base_entry.value() {
            crate::memory::Pager::page_out_semantic(base_pipe, chunks);
        }

        drop(base_entry);

        if delete_branch {
            self.memory_pipes.remove(branch_pipe);
            crate::kprintln!(
                "-> [CoW MERGE] Merged {} chunks from '{}' into '{}' and removed branch.",
                delta_count,
                branch_pipe,
                base_pipe
            );
        } else {
            crate::kprintln!(
                "-> [CoW MERGE] Merged {} chunks from '{}' into '{}'.",
                delta_count,
                branch_pipe,
                base_pipe
            );
        }

        Ok(delta_count)
    }

    fn ensure_pipe_loaded(&self, pipe_name: &str) -> Result<(), String> {
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
                return Err(format!("Pipe '{}' does not exist.", pipe_name));
            }
        }
        Ok(())
    }

    pub fn get_pipe_contents(&self, pipe_name: &str) -> Option<VecDeque<Arc<MemoryChunk>>> {
        let _ = self.ensure_pipe_loaded(pipe_name);

        // Copy out under the shard guard, then release it before resolving a branch's
        // base pipe: paging the base in is a write, which would deadlock if both keys
        // live in the same DashMap shard while this read guard is held.
        let (own_chunks, base_pipe) = {
            let pipe = self.memory_pipes.get(pipe_name)?;
            match pipe.value() {
                PipeState::Active { chunks, .. } => (chunks.clone(), None),
                PipeState::Branched {
                    base_pipe,
                    delta_chunks,
                    ..
                } => (delta_chunks.clone(), Some(base_pipe.clone())),
                PipeState::ActiveSq8Nvme { .. } | PipeState::ActivePqRecompute { .. } => {
                    let restored =
                        crate::memory::Pager::page_in_semantic(pipe_name).unwrap_or_default();
                    (restored, None)
                }
                PipeState::PagedToDisk => return None,
            }
        };

        match base_pipe {
            None => Some(own_chunks),
            Some(base) => {
                let mut unified = self.get_pipe_contents(&base).unwrap_or_default();
                unified.extend(own_chunks);
                Some(unified)
            }
        }
    }

    /// Keeps the `limit` highest-scoring chunks in a min-heap (root = current lowest).
    fn keep_top(
        heap: &mut BinaryHeap<ScoredChunk>,
        limit: usize,
        score: f32,
        chunk: &Arc<MemoryChunk>,
    ) {
        if heap.len() < limit {
            heap.push(ScoredChunk {
                score,
                chunk: Arc::clone(chunk),
            });
        } else if let Some(mut root) = heap.peek_mut()
            && score > root.score
        {
            *root = ScoredChunk {
                score,
                chunk: Arc::clone(chunk),
            };
        }
    }

    /// Highest score first; equal scores are ordered by chunk id so results are deterministic.
    fn into_ranked(heap: BinaryHeap<ScoredChunk>) -> Vec<(f32, Arc<MemoryChunk>)> {
        let mut ranked: Vec<_> = heap.into_iter().map(|s| (s.score, s.chunk)).collect();
        ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        ranked
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
    ) -> SearchOutcome {
        let empty = || SearchOutcome {
            results: Vec::new(),
            top_dense_score: 0.0,
        };

        if top_k == 0 {
            return empty();
        }

        // PAGE FAULT RECOVERY: restore the pipe from SSD if it was paged out
        if self.ensure_pipe_loaded(pipe_name).is_err() {
            return empty();
        }

        // Snapshot the pipe state and release the DashMap lock immediately
        let (pipe_state_snapshot, base_pipe, bm25_scores) = {
            let Some(pipe) = self.memory_pipes.get(pipe_name) else {
                return empty();
            };
            match pipe.value() {
                PipeState::Active { chunks, bm25, .. } => (
                    PipeState::Active {
                        is_persistent: true,
                        chunks: chunks.clone(),
                        bm25: Bm25Index::new(),
                    },
                    None,
                    enable_hybrid.then(|| bm25.score_query(query_text)),
                ),
                PipeState::ActiveSq8Nvme {
                    sq8_chunks,
                    bm25,
                    vector_dim,
                } => (
                    PipeState::ActiveSq8Nvme {
                        sq8_chunks: sq8_chunks.clone(),
                        bm25: Bm25Index::new(),
                        vector_dim: *vector_dim,
                    },
                    None,
                    enable_hybrid.then(|| bm25.score_query(query_text)),
                ),
                PipeState::ActivePqRecompute {
                    codebook,
                    pq_chunks,
                    bm25,
                } => (
                    PipeState::ActivePqRecompute {
                        codebook: Arc::clone(codebook),
                        pq_chunks: pq_chunks.clone(),
                        bm25: Bm25Index::new(),
                    },
                    None,
                    enable_hybrid.then(|| bm25.score_query(query_text)),
                ),
                PipeState::Branched {
                    base_pipe,
                    delta_chunks,
                    bm25,
                } => (
                    PipeState::Branched {
                        base_pipe: base_pipe.clone(),
                        delta_chunks: delta_chunks.clone(),
                        bm25: Bm25Index::new(),
                    },
                    Some(base_pipe.clone()),
                    enable_hybrid.then(|| bm25.score_query(query_text)),
                ),
                PipeState::PagedToDisk => return empty(),
            }
        };

        let passes_filters = |app: &str, cat: MemoryCategory| {
            filter_app.is_none_or(|a| a == app) && filter_category.is_none_or(|c| c == cat)
        };

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Hybrid search fuses two candidate pools; each must hold at least top_k.
        let candidate_limit = if enable_hybrid {
            (top_k * 4).clamp(32, 200).max(top_k)
        } else {
            top_k
        };

        // Mode C (DiskANN) Support: Scans SQ8 in RAM and fetches exact candidates from SSD
        if let PipeState::ActiveSq8Nvme {
            sq8_chunks,
            vector_dim,
            ..
        } = pipe_state_snapshot
        {
            let dim = vector_dim;
            let query_sq8 = Sq8Vector::quantize(query_vector);
            let store = DiskVectorStore::new(pipe_name);

            // Step 1: Scan int8 vectors in RAM
            let mut sq8_candidates: Vec<Sq8Candidate> = sq8_chunks
                .iter()
                .filter(|c| passes_filters(&c.source_app, c.category))
                .map(|c| {
                    let score = Sq8Vector::dot_product(&c.sq8_vector.data, &query_sq8.data);
                    Sq8Candidate {
                        score,
                        f32_offset: c.f32_offset,
                        text: Arc::clone(&c.text),
                        context_payload: Arc::clone(&c.context_payload),
                        source_app: c.source_app.clone(),
                        timestamp: c.timestamp,
                        category: c.category,
                    }
                })
                .collect();

            sq8_candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
            sq8_candidates.truncate(candidate_limit);

            // Step 2 & 3: NVMe Direct seek + Exact SIMD dot-product rerank
            let mut exact_reranked = Vec::new();
            for (i, cand) in sq8_candidates.into_iter().enumerate() {
                if let Ok(f32_vec) = store.read_vector(cand.f32_offset, dim) {
                    let exact_score = Self::dot_product(&f32_vec, query_vector);
                    exact_reranked.push((
                        exact_score,
                        Arc::new(MemoryChunk {
                            id: i as u64,
                            text: cand.text,
                            context_payload: cand.context_payload,
                            vector: Arc::new(f32_vec),
                            source_app: cand.source_app,
                            timestamp: cand.timestamp,
                            category: cand.category,
                        }),
                    ));
                }
            }

            exact_reranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
            let top_dense_score = exact_reranked.first().map_or(0.0, |(s, _)| *s);
            exact_reranked.truncate(top_k);

            return SearchOutcome {
                results: exact_reranked,
                top_dense_score,
            };
        }

        // ── MODE B: LEANN PRODUCT QUANTIZATION SEARCH PATH ───────────────────
        if let PipeState::ActivePqRecompute {
            codebook,
            pq_chunks,
            ..
        } = pipe_state_snapshot
        {
            let dist_table = codebook.compute_distance_table(query_vector);

            let mut pq_candidates: Vec<(f32, Arc<PqChunk>)> = pq_chunks
                .iter()
                .filter(|c| passes_filters(&c.source_app, c.category))
                .map(|c| {
                    let score = codebook.score_pq(&c.pq_codes, &dist_table);
                    (score, Arc::clone(c))
                })
                .collect();

            pq_candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
            pq_candidates.truncate(top_k);

            // Return candidates for candidate recompute
            let dummy_results: Vec<(f32, Arc<MemoryChunk>)> = pq_candidates
                .into_iter()
                .map(|(score, c)| {
                    (
                        score,
                        Arc::new(MemoryChunk {
                            id: c.id,
                            text: Arc::clone(&c.text),
                            context_payload: Arc::clone(&c.context_payload),
                            vector: Arc::new(Vec::new()),
                            source_app: c.source_app.clone(),
                            timestamp: c.timestamp,
                            category: c.category,
                        }),
                    )
                })
                .collect();

            let top_dense_score = dummy_results.first().map_or(0.0, |(s, _)| *s);
            return SearchOutcome {
                results: dummy_results,
                top_dense_score,
            };
        }

        let unified_chunks = match base_pipe {
            None => match pipe_state_snapshot {
                PipeState::Active { chunks, .. } => chunks,
                _ => VecDeque::new(),
            },
            Some(base) => {
                let mut combined = self.get_pipe_contents(&base).unwrap_or_default();
                if let PipeState::Branched { delta_chunks, .. } = pipe_state_snapshot {
                    combined.extend(delta_chunks);
                }
                combined
            }
        };

        // ── PASS 1: Dense SIMD Scoring ──────────────────────────────────────────
        let mut dense_heap = BinaryHeap::with_capacity(candidate_limit);

        for chunk in unified_chunks
            .iter()
            .filter(|c| passes_filters(&c.source_app, c.category))
        {
            let mut score = Self::dot_product(&chunk.vector, query_vector);

            if allow_time_decay {
                let hours_old = current_time.saturating_sub(chunk.timestamp) as f32 / 3600.0;
                score *= (1.0 - hours_old * 0.01).clamp(0.5, 1.0);
            }

            // NaN/inf (e.g. a corrupt vector) would poison the ordering
            if score.is_finite() {
                Self::keep_top(&mut dense_heap, candidate_limit, score, chunk);
            }
        }

        let dense_ranked = Self::into_ranked(dense_heap);
        let top_dense_score = dense_ranked.first().map_or(0.0, |(score, _)| *score);

        let Some(bm25_scores) = bm25_scores else {
            // Dense-only search: the dense ranking is the answer.
            return SearchOutcome {
                results: dense_ranked,
                top_dense_score,
            };
        };

        // ── PASS 2: BM25 Sparse Scoring ─────────────────────────────────────────
        let mut sparse_heap = BinaryHeap::with_capacity(candidate_limit);

        for chunk in unified_chunks
            .iter()
            .filter(|c| passes_filters(&c.source_app, c.category))
        {
            if let Some(&score) = bm25_scores.get(&chunk.id)
                && score > 0.0
                && score.is_finite()
            {
                Self::keep_top(&mut sparse_heap, candidate_limit, score, chunk);
            }
        }

        let sparse_ranked = Self::into_ranked(sparse_heap);

        // ── PASS 3: Reciprocal Rank Fusion (RRF) ────────────────────────────────
        // score = Σ 1 / (k + rank), rank starting at 1 for the best hit in each list.
        const RRF_K: f32 = 60.0;
        let mut fused: HashMap<u64, (f32, Arc<MemoryChunk>)> = HashMap::new();

        for ranked in [dense_ranked, sparse_ranked] {
            for (rank_idx, (_, chunk)) in ranked.into_iter().enumerate() {
                let entry = fused.entry(chunk.id).or_insert((0.0, chunk));
                entry.0 += 1.0 / (RRF_K + (rank_idx + 1) as f32);
            }
        }

        // At most 2 × candidate_limit entries, so a full sort is cheap.
        let mut results: Vec<_> = fused.into_values().collect();
        results.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        results.truncate(top_k);

        SearchOutcome {
            results,
            top_dense_score,
        }
    }

    // ─── RAPTOR HIERARCHICAL RECURSIVE TREE CLUSTERING ───────────────────────────

    /// Builds a RAPTOR summary tree over an existing pipe. Clusters related chunks,
    /// calls the local LLM closure to generate abstractive summaries, embeds them,
    /// and inserts them back into the pipe with Level tags (e.g. [RAPTOR-L1]).
    pub async fn build_raptor_tree<F, Fut>(
        &self,
        pipe_name: &str,
        cluster_size: usize,
        max_levels: usize,
        summarize_fn: F,
    ) -> Result<(usize, usize), String>
    where
        F: Fn(String) -> Fut + Copy,
        Fut: std::future::Future<Output = String>,
    {
        let chunks = self
            .get_pipe_contents(pipe_name)
            .ok_or_else(|| format!("Pipe '{}' not found.", pipe_name))?;

        if chunks.len() < cluster_size {
            return Ok((0, 0));
        }

        let cluster_size = cluster_size.clamp(2, 10);
        let mut current_level_chunks: Vec<Arc<MemoryChunk>> = chunks.into_iter().collect();
        let mut total_summaries = 0;
        let mut levels_built = 0;

        for level in 1..=max_levels {
            if current_level_chunks.len() < 2 {
                break;
            }

            let mut clusters: Vec<Vec<Arc<MemoryChunk>>> = Vec::new();
            let mut visited = vec![false; current_level_chunks.len()];

            // Greedy Cosine Cluster Partitioning
            for i in 0..current_level_chunks.len() {
                if visited[i] {
                    continue;
                }
                visited[i] = true;
                let mut cluster = vec![Arc::clone(&current_level_chunks[i])];

                for j in (i + 1)..current_level_chunks.len() {
                    if visited[j] {
                        continue;
                    }
                    let sim = Self::dot_product(
                        &current_level_chunks[i].vector,
                        &current_level_chunks[j].vector,
                    );
                    if sim > 0.40 {
                        visited[j] = true;
                        cluster.push(Arc::clone(&current_level_chunks[j]));
                        if cluster.len() >= cluster_size {
                            break;
                        }
                    }
                }
                clusters.push(cluster);
            }

            let next_level_chunks = Vec::new();

            for cluster in clusters {
                if cluster.is_empty() {
                    continue;
                }
                let cluster_text = cluster
                    .iter()
                    .map(|c| c.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                let prompt = format!(
                    "Synthesize the following documents into a coherent summary paragraph capturing the overarching themes:\n\n{}",
                    cluster_text
                );

                let summary = summarize_fn(prompt).await;
                let trimmed = summary.trim().to_string();

                if !trimmed.is_empty() {
                    let tagged_text = format!("[RAPTOR-L{}] {}", level, trimmed);
                    let dummy_vec = cluster[0].vector.as_ref().clone();

                    self.write_chunk(
                        pipe_name,
                        tagged_text.clone(),
                        tagged_text,
                        dummy_vec,
                        &format!("raptor_l{}", level),
                        true,
                        MemoryCategory::Fact,
                    );

                    total_summaries += 1;
                }
            }

            levels_built += 1;
            if next_level_chunks.is_empty() {
                break;
            }
            current_level_chunks = next_level_chunks;
        }

        crate::kprintln!(
            "-> [RAPTOR] Tree built for '{}': {} summaries across {} levels.",
            pipe_name,
            total_summaries,
            levels_built
        );

        Ok((total_summaries, levels_built))
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

                    PipeState::ActiveSq8Nvme { sq8_chunks, .. } => {
                        let initial = sq8_chunks.len();
                        sq8_chunks.retain(|chunk| {
                            current_time.saturating_sub(chunk.timestamp) < self.pipe_ttl_secs
                        });
                        let removed = initial - sq8_chunks.len();
                        if removed > 0 {
                            chunks_swept += removed;
                        }
                    }

                    PipeState::ActivePqRecompute { pq_chunks, .. } => {
                        let initial = pq_chunks.len();
                        pq_chunks.retain(|chunk| {
                            current_time.saturating_sub(chunk.timestamp) < self.pipe_ttl_secs
                        });
                        let removed = initial - pq_chunks.len();
                        if removed > 0 {
                            chunks_swept += removed;
                        }
                    }

                    PipeState::Branched { delta_chunks, .. } => {
                        let initial = delta_chunks.len();
                        delta_chunks.retain(|chunk| {
                            current_time.saturating_sub(chunk.timestamp) < self.pipe_ttl_secs
                        });
                        let removed = initial - delta_chunks.len();
                        if removed > 0 {
                            chunks_swept += removed;
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
                PipeState::ActiveSq8Nvme { sq8_chunks, .. } => !sq8_chunks.is_empty(),
                PipeState::ActivePqRecompute { pq_chunks, .. } => !pq_chunks.is_empty(),
                PipeState::Branched { delta_chunks, .. } => !delta_chunks.is_empty(),
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
