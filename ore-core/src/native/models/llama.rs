use crate::native::engine::{ModelConfig, OreEngine};
use anyhow::Result;
use candle_core::Device;
use candle_core::quantized::gguf_file;
use candle_transformers::models::quantized_llama::ModelWeights as LlamaModel;
use std::io::{Read, Seek};
use tokenizers::Tokenizer;

pub fn load<R: Read + Seek>(
    model_name: &str,
    model_content: gguf_file::Content,
    reader: &mut R,
    device: &Device,
    tokenizer: &Tokenizer,
) -> Result<(OreEngine, ModelConfig)> {
    let m = LlamaModel::from_gguf(model_content, reader, device)?;

    let mut stop_tokens = vec![128001, 128009];
    if let Some(id) = tokenizer.token_to_id("<|eot_id|>") {
        stop_tokens.push(id);
    }
    if let Some(id) = tokenizer.token_to_id("<|end_of_text|>") {
        stop_tokens.push(id);
    }
    if let Some(id) = tokenizer.token_to_id("</s>") {
        stop_tokens.push(id);
    }

    let name_lower = model_name.to_lowercase();
    let is_llama_2 = name_lower.contains("llama2") || name_lower.contains("llama-2");

    let formatter: fn(&str) -> String = if name_lower.contains("-base") {
        // base model formatter (no special tokens, just pass through)
        |prompt| prompt.to_string()
    } else if is_llama_2 {
        // llama 2 formatter
        |prompt| {
            format!(
                "<s>[INST] <<SYS>>\nYou are a helpful AI.\n<</SYS>>\n\n{} [/INST]",
                prompt
            )
        }
    } else {
        // Llama 3, 3.1, 3.2, 3.3, and 4
        |prompt| {
            format!(
                "<|start_header_id|>system<|end_header_id|>\n\nYou are a helpful AI.<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                prompt
            )
        }
    };

    let cfg = ModelConfig {
        architecture: "llama".to_string(),
        stop_tokens,
        formatter,
    };

    Ok((OreEngine::Llama(m), cfg))
}
