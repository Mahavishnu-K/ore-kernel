use crate::native::engine::{ModelConfig, OreEngine};
use anyhow::Result;
use candle_core::Device;
use candle_core::quantized::gguf_file;
use candle_transformers::models::quantized_qwen2::ModelWeights as QwenModel;
use std::fs::File;
use tokenizers::Tokenizer;

pub fn load(
    model_name: &str,
    model_content: gguf_file::Content,
    file: &mut File,
    device: &Device,
    tokenizer: &Tokenizer,
) -> Result<(OreEngine, ModelConfig)> {
    let m = QwenModel::from_gguf(model_content, file, device)?;

    let mut stop_tokens = vec![151645, 151643];
    if let Some(id) = tokenizer.token_to_id("<|im_end|>") { stop_tokens.push(id); }
    if let Some(id) = tokenizer.token_to_id("<|endoftext|>") { stop_tokens.push(id); }

    let name_lower = model_name.to_lowercase();

    let formatter: fn(&str) -> String = if name_lower.contains("-base") {
        // base model formatter (no special tokens, just pass through)
        |prompt| prompt.to_string()
    } else {
        |prompt| {
            format!(
                "<|im_start|>system\nYou are a helpful AI assistant.<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                prompt
            )
        }
    };

    let cfg = ModelConfig {
        architecture: "qwen2".to_string(),
        stop_tokens,
        formatter,
    };

    Ok((OreEngine::Qwen(m), cfg))
}