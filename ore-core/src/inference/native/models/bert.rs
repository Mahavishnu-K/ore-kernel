use super::embedding_batch::{embed_in_batches, l2_normalize, mean_pooling};
use anyhow::{Error as E, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use std::fs;
use std::path::Path;
use tokenizers::{Tokenizer, TruncationParams};

pub struct SystemEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl SystemEmbedder {
    /// Loads the BERT architecture natively from Safetensors
    pub fn load(model_dir: &str, device: &Device) -> Result<Self> {
        let safetensors_path = format!("{}/model.safetensors", model_dir);
        let config_path = format!("{}/config.json", model_dir);
        let tokenizer_path = format!("{}/tokenizer.json", model_dir);

        if !Path::new(&safetensors_path).exists() {
            return Err(E::msg(format!(
                "Embedder weights missing. Ensure {} exists.",
                safetensors_path
            )));
        }

        kprintln!("-> [BERT] Loading System Embedder Dictionary...");
        let mut tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(E::msg)?;

        kprintln!("-> [BERT] Loading Configuration...");
        let config_str = fs::read_to_string(&config_path)?;
        let config: Config = serde_json::from_str(&config_str)?;

        // embed_in_batches pads each batch itself. BERT has learned position
        // embeddings, so anything past max_position_embeddings would index out of range.
        let tp = TruncationParams {
            max_length: config.max_position_embeddings,
            ..Default::default()
        };
        tokenizer
            .with_padding(None)
            .with_truncation(Some(tp))
            .map_err(E::msg)?;

        kprintln!("-> [BERT] Loading Neural Weights into RAM...");
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&safetensors_path], DType::F32, device)?
        };
        let model = BertModel::load(vb, &config)?;

        Ok(Self {
            model,
            tokenizer,
            device: device.clone(),
        })
    }

    /// Embeds `texts` in length-bucketed, padded batches (see `embedding_batch`).
    /// Returns one unit-length vector per input, in input order.
    pub fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        embed_in_batches(&self.tokenizer, &self.device, texts, |ids, mask| {
            let token_type_ids = Tensor::zeros_like(ids)?;
            // The mask keeps padding out of attention; mean_pooling keeps it out of the average.
            let hidden = self.model.forward(ids, &token_type_ids, Some(mask))?;
            l2_normalize(&mean_pooling(&hidden, mask)?)
        })
    }
}
