use anyhow::{Error as E, Result};
use candle_core::Tensor;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama::ModelWeights as LlamaModel;
use candle_transformers::models::quantized_qwen2::ModelWeights as QwenModel;
use tokenizers::Tokenizer;

// Supports multiple architectures
pub enum OreEngine {
    Qwen(QwenModel),
    Llama(LlamaModel),
}

impl OreEngine {
    pub fn forward(&mut self, input: &Tensor, start_pos: usize) -> Result<Tensor> {
        match self {
            OreEngine::Qwen(m) => m.forward(input, start_pos).map_err(E::msg),
            OreEngine::Llama(m) => m.forward(input, start_pos).map_err(E::msg),
        }
    }
}

#[derive(Clone)]
pub struct ModelConfig {
    pub architecture: String,
    pub stop_tokens: Vec<u32>,
    pub formatter: fn(&str) -> String,
}

pub struct ActiveEngine {
    pub model: OreEngine,
    pub tokenizer: Tokenizer,
    pub logits_processor: LogitsProcessor,
    pub model_name: String,
    pub config: ModelConfig,
}
