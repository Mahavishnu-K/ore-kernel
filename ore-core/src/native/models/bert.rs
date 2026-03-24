use anyhow::{Error as E, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use std::fs;
use std::path::Path;
use tokenizers::{Tokenizer, PaddingParams, TruncationParams};

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

        println!("-> [BERT] Loading System Embedder Dictionary...");
        let mut tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(E::msg)?;

        let pp = PaddingParams { strategy: tokenizers::PaddingStrategy::BatchLongest, ..Default::default() };
        let tp = TruncationParams { max_length: 8192, ..Default::default() };
        tokenizer.with_padding(Some(pp)).with_truncation(Some(tp)).map_err(E::msg)?;

        println!("-> [BERT] Loading Configuration...");
        let config_str = fs::read_to_string(&config_path)?;
        let config: Config = serde_json::from_str(&config_str)?;

        println!("-> [BERT] Loading Neural Weights into RAM...");
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

    /// Converts English text into a normalized 768-dimension mathematical vector
    pub fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let mut all_vectors = Vec::new();

        for text in texts {
            let tokens = self.tokenizer.encode(text, true).map_err(E::msg)?;
            let token_ids = tokens.get_ids().to_vec();

            // Format into Tensors
            let token_tensor = Tensor::new(token_ids.as_slice(), &self.device)?.unsqueeze(0)?;
            let token_type_ids = Tensor::zeros_like(&token_tensor)?;

            // 1. THE FORWARD PASS
            // We pass `None` because batch_size=1 means no internal padding is needed!
            let embeddings = self.model.forward(&token_tensor, &token_type_ids, None)?;

            // 2. MASKED MEAN POOLING
            // Create the mask [1, seq_len]
            let mask = tokens.get_attention_mask().to_vec();
            let mask_tensor = Tensor::new(mask.as_slice(), &self.device)?.unsqueeze(0)?;

            // Explicitly add the 3rd dimension:[1, seq_len] -> [1, seq_len, 1]
            let mask_f32 = mask_tensor.to_dtype(DType::F32)?.unsqueeze(2)?;

            // Broadcast safe: [1, seq_len, 1] to [1, seq_len, 384]
            let mask_broadcast = mask_f32.broadcast_as(embeddings.shape())?;

            let masked_embeddings = (&embeddings * &mask_broadcast)?;
            let sum_embeddings = masked_embeddings.sum(1)?;

            // Sum the mask: [1, seq_len, 1] -> [1, 1]
            let sum_mask = mask_f32.sum(1)?.clamp(1e-9, f64::MAX)?;
            
            let mean_pooled = sum_embeddings.broadcast_div(&sum_mask)?;

            // 3. L2 NORMALIZATION
            let sum_sq = mean_pooled.sqr()?.sum_keepdim(1)?;
            let norm = sum_sq.sqrt()?;
            let normalized = mean_pooled.broadcast_div(&norm)?;

            let final_vector = normalized.squeeze(0)?.to_vec1::<f32>()?;
            all_vectors.push(final_vector);
        }

        Ok(all_vectors)
    }
}