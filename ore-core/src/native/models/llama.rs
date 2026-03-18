use crate::native::engine::{ModelConfig, OreBrain};
use anyhow::Result;
use candle_core::Device;
use candle_core::quantized::gguf_file;
use candle_transformers::models::quantized_llama::ModelWeights as LlamaModel;
use std::fs::File;
use tokenizers::Tokenizer;

pub fn load(
    model_content: gguf_file::Content,
    file: &mut File,
    device: &Device,
    tokenizer: &Tokenizer,
) -> Result<(OreBrain, ModelConfig)> {
    let m = LlamaModel::from_gguf(model_content, file, device)?;

    let mut stop_tokens = vec![128001, 128009];
    if let Some(id) = tokenizer.token_to_id("<|eot_id|>") { stop_tokens.push(id); }
    if let Some(id) = tokenizer.token_to_id("<|end_of_text|>") { stop_tokens.push(id); }

    let cfg = ModelConfig {
        architecture: "llama".to_string(),
        stop_tokens,
        formatter: |prompt| {
            format!(
                "<|start_header_id|>system<|end_header_id|>\n\nYou are a helpful AI.<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                prompt
            )
        },
    };
    Ok((OreBrain::Llama(m), cfg))
}