use crate::native::engine::{ModelConfig, OreBrain};
use anyhow::Result;
use candle_core::Device;
use candle_core::quantized::gguf_file;
use candle_transformers::models::quantized_qwen2::ModelWeights as QwenModel;
use std::fs::File;
use tokenizers::Tokenizer;

pub fn load(
    model_content: gguf_file::Content,
    file: &mut File,
    device: &Device,
    tokenizer: &Tokenizer,
) -> Result<(OreBrain, ModelConfig)> {
    let m = QwenModel::from_gguf(model_content, file, device)?;

    let mut stop_tokens = vec![151645, 151643];
    if let Some(id) = tokenizer.token_to_id("<|im_end|>") { stop_tokens.push(id); }

    let cfg = ModelConfig {
        architecture: "qwen2".to_string(),
        stop_tokens,
        formatter: |prompt| {
            format!("<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n", prompt)
        },
    };
    Ok((OreBrain::Qwen(m), cfg))
}