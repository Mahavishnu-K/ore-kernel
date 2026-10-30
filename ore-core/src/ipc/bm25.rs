use super::chunking::MemoryChunk;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

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

    /// Removes an evicted chunk from the inverted index postings and document statistics.
    /// Prevents memory leaks and maintains accurate average document length (avgdl) and IDF calculations.
    pub fn remove_chunk(&mut self, chunk_id: u64, text: &str) {
        let tokens = tokenize_bm25(text);
        let unique_terms: HashSet<_> = tokens.into_iter().collect();

        for term in unique_terms {
            if let Some(postings) = self.inverted_index.get_mut(&term) {
                // Remove the posting for this specific chunk_id
                postings.retain(|&(id, _)| id != chunk_id);

                // Clean up empty dictionary keys so the hashmap doesn't grow with empty vectors
                if postings.is_empty() {
                    self.inverted_index.remove(&term);
                }
            }
        }

        // Subtract the document's tokens from the total corpus token count
        if let Some(doc_len) = self.doc_lengths.remove(&chunk_id) {
            self.total_tokens = self.total_tokens.saturating_sub(doc_len);
        }
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
