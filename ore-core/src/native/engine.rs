use crate::memory::ContextMessage;
use crate::native::gguf_tokenizer::TokenizerFromGguf;
use crate::native::models;
use crate::native::models::llama::ModelWeights as LlamaModel;
use crate::native::models::qwen2::ModelWeights as Qwen2Model;
use crate::native::models::qwen3::ModelWeights as Qwen3Model;
use crate::native::models::qwen3_moe::GGUFQWenMoE as Qwen3MoeModel;
use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use memmap2::Mmap;
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

pub struct ActiveEngine {
    pub model: OreEngine,
    pub tokenizer: Tokenizer,
    pub logits_processor: LogitsProcessor,
    pub model_name: String,
    pub config: ModelConfig,
    pub current_app_id: String,
    pub stateful_paging: bool,
    pub last_used: std::time::Instant,
    pub _mmap: memmap2::Mmap,
}

impl ActiveEngine {
    /// The ultra-fast, zero-copy GGUF loader using OS-level memory mapping (mmap)
    pub fn load(
        model_name: &str,
        app_id: &str,
        stateful_paging: bool,
        device: &Device,
    ) -> Result<Self> {
        let safe_folder_name = model_name.replace(":", "-");
        let model_dir = crate::get_ore_dir().join("models").join(&safe_folder_name);
        let gguf_path = model_dir.join("model.gguf");
        let local_tokenizer_path = model_dir.join("tokenizer.json");

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
            _mmap: mmap,
        })
    }
}
