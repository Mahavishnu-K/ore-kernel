pub mod engine;
pub mod gguf_tokenizer;
pub mod models;

use crate::driver::{DriverError, InferenceDriver, LocalModel, VramProcess};
use crate::swap::ContextMessage;
use anyhow::{Error as E, Result};
use async_trait::async_trait;
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use engine::ActiveEngine;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use tokenizers::Tokenizer;
use tokio::sync::mpsc::UnboundedSender;
use gguf_tokenizer::TokenizerFromGguf;

pub struct NativeDriver {
    engine: Arc<StdMutex<Option<ActiveEngine>>>,
    device: Device,
}

impl NativeDriver {
    pub fn new() -> Self {
        println!("-> [CANDLE] Probing Motherboard for Hardware Compute...");
        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else if candle_core::utils::metal_is_available() {
            Device::new_metal(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };
        Self { engine: Arc::new(StdMutex::new(None)), device }
    }

    fn load_weights_into_memory(model_name: &str, device: &Device) -> Result<ActiveEngine> {
        let safe_folder_name = model_name.replace(":", "-");
        let model_dir = format!("../models/{}", safe_folder_name);
        let gguf_path = format!("{}/model.gguf", model_dir);
        let local_tokenizer_path = format!("{}/tokenizer.json", model_dir);

        if !Path::new(&gguf_path).exists() {
            return Err(E::msg(format!("Files not found. Run 'ore pull {}'", model_name)));
        }

        println!("-> [CANDLE] Reading GGUF Headers...");
        let mut file = File::open(&gguf_path)?;
        let model_content = gguf_file::Content::read(&mut file).map_err(E::msg)?;
        
        let arch_name = match model_content.metadata.get("general.architecture") {
            Some(gguf_file::Value::String(arch)) => arch.clone(),
            _ => "unknown".to_string(),
        };
        println!("-> [CANDLE] Detected Architecture: '{}'", arch_name);

        let global_tokenizer_name = if model_name.to_lowercase().contains("qwen2.5") {
            "qwen2.5"
        } else if model_name.to_lowercase().contains("llama4") || model_name.to_lowercase().contains("llama-4") {
            "llama4"
        } else if model_name.to_lowercase().contains("llama3.3") || model_name.to_lowercase().contains("llama-3.3") {
            "llama3.3"
        } else if model_name.to_lowercase().contains("llama3.2") || model_name.to_lowercase().contains("llama3") || model_name.to_lowercase().contains("llama-3.2") || model_name.to_lowercase().contains("llama-3") {
            "llama3.2" 
        } else if model_name.to_lowercase().contains("llama2") || model_name.to_lowercase().contains("llama-2") {
            "llama2"
        } else if model_name.to_lowercase().contains("codellama") {
            "codellama"
        } else {
            arch_name.as_str()  
        };

        let global_path = format!("../tokenizers/{}.json", global_tokenizer_name);

        // universal tokenizer fallback
        let tokenizer = if Path::new(&local_tokenizer_path).exists() {
            println!("->[CANDLE] Using Local Dictionary...");
            Tokenizer::from_file(&local_tokenizer_path).map_err(E::msg)?
        } else if Path::new(&global_path).exists() {
            println!("->[CANDLE] Local dictionary not found. Using Universal OS Dictionary for '{}'...", arch_name);
            Tokenizer::from_file(&global_path).map_err(E::msg)?
        } else {
            // THE RAW GGUF EXTRACTOR
            println!("-> [CANDLE] [WARN] No JSON found. Extracting Tokenizer directly from GGUF metadata...");
            let tok_file = File::open(&gguf_path)?;
            let mut reader = std::io::BufReader::new(tok_file);
            let content = gguf_file::Content::read(&mut reader).map_err(E::msg)?;
            
            let extracted_tokenizer = Tokenizer::from_gguf(&content).map_err(E::msg)?;

            // SAVE IT TO DISK
            println!("-> [CANDLE] JIT Cache: Saving extracted dictionary to {}...", local_tokenizer_path);
            if let Err(e) = extracted_tokenizer.save(&local_tokenizer_path, true) {
                println!("-> [CANDLE] [WARN] Could not save cached tokenizer: {}", e);
            } else {
                println!("-> [CANDLE] [SUCCESS] Dictionary permanently cached.");
            }

            extracted_tokenizer
        };

        // architecture router
        let (model, config) = match arch_name.as_str() {
            "llama" => models::llama::load(model_name, model_content, &mut file, device, &tokenizer)?,
            "qwen2" => models::qwen::load(model_name, model_content, &mut file, device, &tokenizer)?,
            _ => return Err(E::msg(format!("Architecture not supported yet: {}", arch_name))),
        };

        let logits_processor = LogitsProcessor::new(299792458, Some(0.7), None);

        Ok(ActiveEngine { 
            model, 
            tokenizer, 
            logits_processor, 
            model_name: model_name.to_string(), 
            config 
        })
    }
}

#[async_trait]
impl InferenceDriver for NativeDriver {
    fn engine_name(&self) -> &'static str { "Native Candle Engine" }

    async fn is_online(&self) -> bool { true }

    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError> {
        let state = self.engine.lock().unwrap();
        if let Some(active) = &*state {
            Ok(vec![VramProcess {
                model_name: active.model_name.clone(), 
                size_bytes: 1024 * 1024 * 1024, 
                size_vram_bytes: 0,
            }])
        } else {
            Ok(vec![])
        }
    }

    async fn preload_model(&self, model: &str) -> Result<(), DriverError> {
        let model = model.trim().replace(":", "-");
        let mut state = self.engine.lock().unwrap();
        if state.is_none() || state.as_ref().unwrap().model_name != model {
            *state = Some(Self::load_weights_into_memory(&model, &self.device).map_err(|e| DriverError::ExecutionFailed(e.to_string()))?);
        }
        Ok(())
    }

    async fn unload_model(&self, _model: &str) -> Result<(), DriverError> {
        let mut state = self.engine.lock().unwrap();
        *state = None; 
        Ok(())
    }

    async fn generate_text(&self, model: &str, prompt: &str, _history: Option<Vec<ContextMessage>>, tx: UnboundedSender<String>) -> Result<(), DriverError> {
        let model = model.trim().replace(':', "-");
        {
            let mut state = self.engine.lock().unwrap();
            if state.is_none() || state.as_ref().unwrap().model_name != model {
                *state = Some(Self::load_weights_into_memory(&model, &self.device).map_err(|e| DriverError::ExecutionFailed(e.to_string()))?);
            }
        }

        let engine_arc = Arc::clone(&self.engine);
        let safe_prompt = prompt.to_string();
        let device_clone = self.device.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut state_guard = engine_arc.lock().unwrap();
            let active = state_guard.as_mut().unwrap();

            let formatted_prompt = (active.config.formatter)(&safe_prompt);
            let mut tokens = active.tokenizer.encode(formatted_prompt, true).unwrap().get_ids().to_vec();

            for index in 0..8192 { 
                let context_size = if index > 0 { 1 } else { tokens.len() };
                let start_pos = tokens.len().saturating_sub(context_size);
                
                let input_tensor = Tensor::new(&tokens[start_pos..], &device_clone).unwrap().unsqueeze(0).unwrap();
                let logits = active.model.forward(&input_tensor, start_pos).unwrap();
                let logits = logits.squeeze(0).unwrap().squeeze(0).unwrap().to_dtype(DType::F32).unwrap();
                
                let next_token_id = active.logits_processor.sample(&logits).unwrap();
                
                if active.config.stop_tokens.contains(&next_token_id) { 
                    break; 
                }

                let word = active.tokenizer.decode(&[next_token_id], true).unwrap();
                
                if tx.send(word).is_err() {
                    break;
                }

                tokens.push(next_token_id);
            }
            Ok(())
        }).await.map_err(|e| DriverError::ExecutionFailed(e.to_string()))?;

        result.map_err(DriverError::ExecutionFailed)
    }

    // will be implemented for dynamic model management
    async fn pull_model(&self, _model: &str) -> Result<(), DriverError> { Ok(()) }
    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError> { Ok(vec![]) }
    async fn generate_embeddings(&self, _m: &str, _i: &str) -> Result<Vec<f32>, DriverError> { Ok(vec![]) }
}