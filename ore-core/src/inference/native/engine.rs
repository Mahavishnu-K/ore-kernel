use crate::inference::native::gguf_tokenizer::TokenizerFromGguf;
use crate::inference::native::models;
use crate::inference::native::models::llama::ModelWeights as LlamaModel;
use crate::inference::native::models::qwen2::ModelWeights as Qwen2Model;
use crate::inference::native::models::qwen3::ModelWeights as Qwen3Model;
use crate::inference::native::models::qwen3_moe::GGUFQWenMoE as Qwen3MoeModel;
use crate::memory::ContextMessage;
use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use memmap2::Mmap;
use ore_common::constants;
use ore_common::paths;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;
use tokenizers::Tokenizer;

// Supports multiple architectures
pub enum OreEngine {
    Qwen2(Qwen2Model),
    Llama(LlamaModel),
    Qwen3(Qwen3Model),
    Qwen3Moe(Qwen3MoeModel),
}

impl OreEngine {
    pub fn forward(&mut self, input: &Tensor, start_pos: usize) -> Result<Tensor> {
        match self {
            OreEngine::Qwen2(m) => m.forward(input, start_pos).map_err(E::msg),
            OreEngine::Llama(m) => m.forward(input, start_pos).map_err(E::msg),
            OreEngine::Qwen3(m) => m.forward(input, start_pos).map_err(E::msg),
            OreEngine::Qwen3Moe(m) => m.forward(input, start_pos).map_err(E::msg),
        }
    }

    pub fn num_layers(&self) -> usize {
        match self {
            OreEngine::Llama(m) => m.layers.len(),
            OreEngine::Qwen2(m) => m.layers.len(),
            OreEngine::Qwen3(m) => m.layers.len(),
            OreEngine::Qwen3Moe(m) => m.layers.len(),
        }
    }

    pub fn clear_kv_cache(&mut self) {
        match self {
            OreEngine::Llama(m) => m.clear_kv_cache(),
            OreEngine::Qwen2(m) => m.clear_kv_cache(),
            OreEngine::Qwen3(m) => m.clear_kv_cache(),
            OreEngine::Qwen3Moe(m) => m.clear_kv_cache(),
        }
    }

    pub fn truncate_kv_cache(&mut self, len: usize) {
        match self {
            OreEngine::Llama(m) => m.truncate_kv_cache(len),
            OreEngine::Qwen2(m) => m.truncate_kv_cache(len),
            OreEngine::Qwen3(m) => m.truncate_kv_cache(len),
            OreEngine::Qwen3Moe(m) => m.truncate_kv_cache(len),
        }
    }

    pub fn get_kv_cache_len(&self) -> usize {
        match self {
            OreEngine::Llama(m) => m.get_kv_cache_len(),
            OreEngine::Qwen2(m) => m.get_kv_cache_len(),
            OreEngine::Qwen3(m) => m.get_kv_cache_len(),
            OreEngine::Qwen3Moe(m) => m.get_kv_cache_len(),
        }
    }

    /// Rips the physical brain state out of the GPU
    pub fn get_kv_cache(&self) -> Vec<Option<(Tensor, Tensor)>> {
        match self {
            OreEngine::Llama(m) => m.get_kv_cache(),
            OreEngine::Qwen2(m) => m.get_kv_cache(),
            OreEngine::Qwen3(m) => m.get_kv_cache(),
            OreEngine::Qwen3Moe(m) => m.get_kv_cache(),
        }
    }

    /// Injects a frozen brain state back into the AI
    pub fn set_kv_cache(&mut self, cache: Vec<Option<(Tensor, Tensor)>>) {
        match self {
            OreEngine::Llama(m) => m.set_kv_cache(cache),
            OreEngine::Qwen2(m) => m.set_kv_cache(cache),
            OreEngine::Qwen3(m) => m.set_kv_cache(cache),
            OreEngine::Qwen3Moe(m) => m.set_kv_cache(cache),
        }
    }
}

#[derive(Clone)]
pub struct ModelConfig {
    pub architecture: String,
    pub stop_tokens: Vec<u32>,
    pub formatter: fn(&[ContextMessage], &str) -> String,
}

/// How many leading tokens of `cached` can be kept when the model is next run on `next`:
/// their common prefix, but never all of `next`, because the last token must be run
/// again to produce logits.
///
/// A KV entry depends only on the tokens at and before its position, so an identical
/// token prefix means identical cached keys and values. Anything after the first
/// differing token (another retrieved context, a compacted history, a re-tokenized
/// answer) is recomputed.
pub fn reusable_prefix(cached: &[u32], next: &[u32]) -> usize {
    let common = cached.iter().zip(next).take_while(|(a, b)| a == b).count();
    common.min(next.len().saturating_sub(1))
}

pub struct ActiveEngine {
    pub model: OreEngine,
    pub tokenizer: Tokenizer,
    pub logits_processor: LogitsProcessor,
    pub model_name: String,
    pub config: ModelConfig,
    pub current_app_id: String,
    pub stateful_paging: bool,
    pub last_used: std::time::Instant,
    /// The token ids the KV cache holds, one per cached position. Kept in step with the
    /// cache by `feed`, `reuse_prefix`, `reset_cache` and `restore_cache`.
    pub cached_tokens: Vec<u32>,
    pub _mmap: memmap2::Mmap,
}

impl ActiveEngine {
    /// Empties the KV cache.
    pub fn reset_cache(&mut self) {
        self.model.clear_kv_cache();
        self.cached_tokens.clear();
    }

    /// Installs a KV cache paged in from SSD together with the token ids it was built from.
    pub fn restore_cache(&mut self, cache: Vec<Option<(Tensor, Tensor)>>, tokens: Vec<u32>) {
        self.model.set_kv_cache(cache);
        self.cached_tokens = tokens;
    }

    /// Prepares the cache for running the model on `tokens`: keeps the longest prefix it
    /// verifiably shares with them (see [`reusable_prefix`]) and drops the rest. Returns how
    /// many tokens were kept; the caller feeds `tokens[kept..]`.
    pub fn reuse_prefix(&mut self, tokens: &[u32]) -> usize {
        // A cache whose contents aren't fully recorded can't be checked, so it isn't trusted.
        if self.model.get_kv_cache_len() != self.cached_tokens.len() {
            self.reset_cache();
            return 0;
        }
        let keep = reusable_prefix(&self.cached_tokens, tokens);
        if keep == 0 {
            self.reset_cache();
        } else if keep < self.cached_tokens.len() {
            self.model.truncate_kv_cache(keep);
            self.cached_tokens.truncate(keep);
        }
        keep
    }

    /// Runs the model on `ids`, appended after the cached tokens, and records them as cached.
    /// Returns the logits for the last position.
    pub fn feed(&mut self, ids: &[u32], device: &Device) -> Result<Tensor> {
        let input = Tensor::new(ids, device)?.unsqueeze(0)?;
        let logits = self.model.forward(&input, self.cached_tokens.len())?;
        self.cached_tokens.extend_from_slice(ids);
        Ok(logits)
    }

    /// The ultra-fast, zero-copy GGUF loader using OS-level memory mapping (mmap)
    pub fn load(
        model_name: &str,
        app_id: &str,
        stateful_paging: bool,
        device: &Device,
    ) -> Result<Self> {
        let safe_folder_name = model_name.replace(":", "-");
        let model_dir = paths::get_models_dir().join(&safe_folder_name);
        let gguf_path = model_dir.join(constants::MODEL_GGUF_FILE_NAME);
        let local_tokenizer_path = model_dir.join(constants::TOKENIZER_JSON_FILE_NAME);

        if !Path::new(&gguf_path).exists() {
            return Err(E::msg(format!(
                "Files not found. Run 'ore pull {}'",
                model_name
            )));
        }

        // 1. Memory Map the Weights
        kprintln!("-> [CANDLE] Allocating Virtual Memory Pointer via mmap...");
        let file = File::open(&gguf_path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let mut cursor = Cursor::new(&mmap[..]);

        // 2. Extract Metadata
        let model_content = gguf_file::Content::read(&mut cursor).map_err(E::msg)?;
        let arch_name = match model_content.metadata.get("general.architecture") {
            Some(gguf_file::Value::String(arch)) => arch.clone(),
            _ => "unknown".to_string(),
        };
        kprintln!("-> [CANDLE] Detected Architecture: '{}'", arch_name);

        // universal tokenizer fallback
        let tokenizer = if Path::new(&local_tokenizer_path).exists() {
            kprintln!("-> [CANDLE] Using Local Dictionary...");
            Tokenizer::from_file(&local_tokenizer_path).map_err(E::msg)?
        } else {
            // THE RAW GGUF EXTRACTOR
            kprintln!(
                "-> [CANDLE] [WARN] No JSON found. Extracting Tokenizer directly from GGUF metadata..."
            );
            let tok_file = File::open(&gguf_path)?;
            let mut reader = std::io::BufReader::new(tok_file);
            let content = gguf_file::Content::read(&mut reader).map_err(E::msg)?;

            let extracted_tokenizer = Tokenizer::from_gguf(&content).map_err(E::msg)?;

            // SAVE IT TO DISK
            kprintln!(
                "-> [CANDLE] JIT Cache: Saving extracted dictionary to {}...",
                local_tokenizer_path.display()
            );
            if let Err(e) = extracted_tokenizer.save(&local_tokenizer_path, true) {
                kprintln!("-> [CANDLE] [WARN] Could not save cached tokenizer: {}", e);
            } else {
                kprintln!("-> [CANDLE] [SUCCESS] Dictionary permanently cached.");
            }

            extracted_tokenizer
        };

        // 4. Load Neural Weights (Architecture Router)
        let (model, config) = match arch_name.as_str() {
            "llama" => {
                models::llama::load(model_name, model_content, &mut cursor, device, &tokenizer)?
            }
            "qwen2" => {
                models::qwen2::load(model_name, model_content, &mut cursor, device, &tokenizer)?
            }
            "qwen3" => {
                models::qwen3::load(model_name, model_content, &mut cursor, device, &tokenizer)?
            }
            "qwen3moe" | "qwen2moe" => {
                models::qwen3_moe::load(model_name, model_content, &mut cursor, device, &tokenizer)?
            }
            _ => {
                return Err(E::msg(format!(
                    "Architecture not supported natively: {}",
                    arch_name
                )));
            }
        };

        let logits_processor = LogitsProcessor::new(299792458, Some(0.7), None);

        Ok(Self {
            model,
            tokenizer,
            logits_processor,
            model_name: model_name.to_string(),
            config,
            current_app_id: app_id.to_string(),
            stateful_paging,
            last_used: std::time::Instant::now(),
            cached_tokens: Vec::new(),
            _mmap: mmap,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::reusable_prefix;

    #[test]
    fn identical_prompt_recomputes_only_the_last_token() {
        assert_eq!(reusable_prefix(&[1, 2, 3, 4], &[1, 2, 3, 4]), 3);
    }

    #[test]
    fn growing_conversation_reuses_everything_cached() {
        assert_eq!(reusable_prefix(&[1, 2, 3], &[1, 2, 3, 4, 5]), 3);
    }

    #[test]
    fn auto_rag_context_is_not_reused_as_history() {
        // cached: [sys, ctx1, ctx1, q1, a1]; next: [sys, q1, a1, ctx2, q2]
        let cached = [7, 100, 101, 20, 30];
        let next = [7, 20, 30, 200, 21];
        assert_eq!(reusable_prefix(&cached, &next), 1);
    }

    #[test]
    fn stale_tail_after_compaction_is_dropped() {
        assert_eq!(reusable_prefix(&[1, 2, 3, 9, 9, 9], &[1, 2, 3, 4]), 3);
    }

    #[test]
    fn shorter_prompt_still_leaves_a_token_to_run() {
        assert_eq!(reusable_prefix(&[1, 2, 3, 4], &[1, 2]), 1);
        assert_eq!(reusable_prefix(&[1, 2, 3], &[1]), 0);
    }

    #[test]
    fn nothing_cached_or_nothing_to_run() {
        assert_eq!(reusable_prefix(&[], &[1, 2, 3]), 0);
        assert_eq!(reusable_prefix(&[1, 2, 3], &[]), 0);
        assert_eq!(reusable_prefix(&[5, 6], &[1, 2, 3]), 0);
    }
}
