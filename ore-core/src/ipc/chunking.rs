use ore_common::protocol::MemoryCategory;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkPayload {
    /// The tight sentence/probe text embedded into vector space and indexed by BM25
    pub probe_text: String,
    /// The surrounding context window payload injected into LLM prompts
    pub context_payload: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: u64,
    pub text: Arc<String>,
    /// Decoupled parent/window context payload
    pub context_payload: Arc<String>,
    /// Pre-normalized unit vector (generated from `text`)
    pub vector: Arc<Vec<f32>>,
    pub source_app: String,
    pub timestamp: u64,
    pub category: MemoryCategory,
}

pub fn create_sliding_windows(text: &str, window_size: usize, overlap: usize) -> Vec<ChunkPayload> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut chunks = Vec::new();
    let mut i = 0;
    let step = std::cmp::max(1, window_size.saturating_sub(overlap));

    while i < words.len() {
        let end = std::cmp::min(i + window_size, words.len());

        let chunk_str = words[i..end].join(" ");
        chunks.push(ChunkPayload {
            probe_text: chunk_str.clone(),
            context_payload: chunk_str,
        });

        if end == words.len() {
            break;
        }
        i += step;
    }
    chunks
}

/// Splits text into natural sentence units and aggregates them up to window_size words
pub fn create_sentence_aware_chunks(
    text: &str,
    target_words: usize,
    overlap_words: usize,
) -> Vec<ChunkPayload> {
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
        return create_sliding_windows(text, target_words, overlap_words);
    }

    let mut chunks = Vec::new();
    let mut current_chunk_sentences: Vec<String> = Vec::new();
    let mut current_word_count = 0;

    for sentence in sentences {
        let s_words = sentence.split_whitespace().count();

        // If a single sentence exceeds target words, fallback to sliding window for that sentence
        if s_words >= target_words {
            if !current_chunk_sentences.is_empty() {
                let full = current_chunk_sentences.join(" ");
                chunks.push(ChunkPayload {
                    probe_text: full.clone(),
                    context_payload: full,
                });
                current_chunk_sentences.clear();
                current_word_count = 0;
            }
            chunks.extend(create_sliding_windows(
                &sentence,
                target_words,
                overlap_words,
            ));
            continue;
        }

        if current_word_count + s_words > target_words && !current_chunk_sentences.is_empty() {
            let full = current_chunk_sentences.join(" ");
            chunks.push(ChunkPayload {
                probe_text: full.clone(),
                context_payload: full,
            });

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
        let full = current_chunk_sentences.join(" ");
        chunks.push(ChunkPayload {
            probe_text: full.clone(),
            context_payload: full,
        });
    }

    chunks
}

/// Decoupled Sentence-Window Chunking:
/// Embeds the isolated, high-precision focal sentence as the probe vector,
/// but expands the surrounding context up to `target_window_words` for the LLM context payload.
pub fn create_sentence_window_chunks(text: &str, target_window_words: usize) -> Vec<ChunkPayload> {
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
        return create_sliding_windows(text, target_window_words, target_window_words / 5);
    }

    let num_sentences = sentences.len();
    let sentence_word_counts: Vec<usize> = sentences
        .iter()
        .map(|s| s.split_whitespace().count())
        .collect();

    let mut chunks = Vec::with_capacity(num_sentences);

    for i in 0..num_sentences {
        let probe_text = sentences[i].clone();

        // Centered window expansion: Expand backward and forward until target_window_words
        let mut start_idx = i;
        let mut end_idx = i;
        let mut total_words = sentence_word_counts[i];

        let mut expanded = true;
        while expanded && total_words < target_window_words {
            expanded = false;

            // Expand backward
            if start_idx > 0 {
                let prev = start_idx - 1;
                start_idx = prev;
                total_words += sentence_word_counts[prev];
                expanded = true;
            }

            // Expand forward
            if total_words < target_window_words && end_idx + 1 < num_sentences {
                let next = end_idx + 1;
                end_idx = next;
                total_words += sentence_word_counts[next];
                expanded = true;
            }
        }

        let context_payload = sentences[start_idx..=end_idx].join(" ");
        chunks.push(ChunkPayload {
            probe_text,
            context_payload,
        });
    }

    chunks
}

/// Splits by double-newlines (\n\n) preserving whole thoughts and paragraphs
pub fn create_paragraph_chunks(text: &str, max_words: usize) -> Vec<ChunkPayload> {
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.is_empty() {
        return create_sliding_windows(text, max_words, max_words / 5);
    }

    let mut chunks = Vec::new();
    let mut current_buffer = Vec::new();
    let mut current_count = 0;

    for p in paragraphs {
        let p_words = p.split_whitespace().count();

        if p_words > max_words {
            if !current_buffer.is_empty() {
                let full = current_buffer.join("\n\n");
                chunks.push(ChunkPayload {
                    probe_text: full.clone(),
                    context_payload: full,
                });
                current_buffer.clear();
                current_count = 0;
            }
            chunks.extend(create_sliding_windows(p, max_words, max_words / 5));
            continue;
        }

        if current_count + p_words > max_words && !current_buffer.is_empty() {
            let full = current_buffer.join("\n\n");
            chunks.push(ChunkPayload {
                probe_text: full.clone(),
                context_payload: full,
            });
            current_buffer.clear();
            current_count = 0;
        }

        current_count += p_words;
        current_buffer.push(p);
    }

    if !current_buffer.is_empty() {
        let full = current_buffer.join("\n\n");
        chunks.push(ChunkPayload {
            probe_text: full.clone(),
            context_payload: full,
        });
    }

    chunks
}

pub fn chunk_text(text: &str, strategy: &str, size: usize, overlap: usize) -> Vec<ChunkPayload> {
    match strategy {
        "sliding_window" => create_sliding_windows(text, size, overlap),
        "sentence_aware" => create_sentence_aware_chunks(text, size, overlap),
        "sentence_window" => create_sentence_window_chunks(text, size),
        "paragraph" => create_paragraph_chunks(text, size),
        "exact_match" => vec![ChunkPayload {
            probe_text: text.to_string(),
            context_payload: text.to_string(),
        }],
        _ => create_sliding_windows(text, size, overlap),
    }
}
