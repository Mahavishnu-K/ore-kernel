use candle_core::Tensor;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama::ModelWeights as LlamaModel;
use candle_transformers::models::quantized_qwen2::ModelWeights as QwenModel;
use anyhow::{Error as E, Result};
use tokenizers::Tokenizer;

// Supports multiple architectures
pub enum OreBrain {
    Qwen(QwenModel),
    Llama(LlamaModel),
}

impl OreBrain {
    pub fn forward(&mut self, input: &Tensor, start_pos: usize) -> Result<Tensor> {
        match self {
            OreBrain::Qwen(m) => m.forward(input, start_pos).map_err(E::msg),
            OreBrain::Llama(m) => m.forward(input, start_pos).map_err(E::msg),
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
    pub model: OreBrain,
    pub tokenizer: Tokenizer,
    pub logits_processor: LogitsProcessor,
    pub model_name: String,
    pub config: ModelConfig,
}