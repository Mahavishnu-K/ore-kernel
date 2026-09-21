pub mod engine;
pub mod gguf_tokenizer;
pub mod kv_manager;
pub mod models;

use crate::driver::{DriverError, InferenceDriver, LocalModel, VramProcess};
use crate::memory::ContextMessage;
use anyhow::Result;
use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use engine::ActiveEngine;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use time::OffsetDateTime;
use time::macros::format_description;
use tokio::sync::mpsc::UnboundedSender;

pub struct NativeDriver {
    engine: Arc<StdMutex<Option<ActiveEngine>>>,
    device: Device,
}

impl Default for NativeDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeDriver {
    pub fn new() -> Self {
        kprintln!("-> [CANDLE] Probing Motherboard for Hardware Compute...");
        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else if candle_core::utils::metal_is_available() {
            Device::new_metal(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };
        kprintln!("-> [CANDLE] Using Device: {:?}", device);
        Self {
            engine: Arc::new(StdMutex::new(None)),
            device,
        }
    }

    fn load_weights_into_memory(
        model_name: &str,
        app_id: &str,
        stateful_paging: bool,
        device: &Device,
    ) -> Result<ActiveEngine> {
        ActiveEngine::load(model_name, app_id, stateful_paging, device)
    }
}

#[async_trait]
impl InferenceDriver for NativeDriver {
    fn engine_name(&self) -> &'static str {
        "Native Candle Engine"
    }

    async fn is_online(&self) -> bool {
        true
    }

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
            *state = Some(
                // We assign the RAM to a dummy agent. The next real agent that
                // runs will instantly "Agent Swap" and take ownership!
                Self::load_weights_into_memory(
                    &model,
                    "system_preloader", // Dummy App ID
                    false,              // No paging needed for a dummy
                    &self.device,
                )
                .map_err(|e| DriverError::ExecutionFailed(e.to_string()))?,
            );
        }
        Ok(())
    }

    async fn unload_model(&self, _model: &str) -> Result<(), DriverError> {
        let mut state = self.engine.lock().unwrap();
        *state = None;
        Ok(())
    }

    async fn generate_text(
        &self,
        model: &str,
        app_id: &str,
        stateful_paging: bool,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
        current_fingerprint: &str,
    ) -> Result<(), DriverError> {
        let model = model.trim().replace(':', "-");

        let mut is_hot_hit = false;
        let mut eviction_task = None;
        // LAZY EVICTION CHECK (RAM -> SSD)
        {
            let mut state = self.engine.lock().unwrap();

            let is_same_model = state.is_some() && state.as_ref().unwrap().model_name == model;
            let is_same_agent = state.is_some() && state.as_ref().unwrap().current_app_id == app_id;

            let needs_model_reload = !is_same_model;
            let needs_agent_swap = is_same_model && !is_same_agent;

            // 1. FREEZE OLD STATE TO SSD (If ANY switch is happening)
            if ((needs_model_reload && state.is_some()) || needs_agent_swap)
                && let Some(active) = state.as_mut()
                && active.stateful_paging
            {
                println!(
                    "-> [NATIVE DRIVER] Lazy Eviction: Freezing Agent '{}' Brain State to SSD...",
                    active.current_app_id
                );
                let raw_cache = active.model.get_kv_cache();
                let flat_tensors =
                    crate::inference::native::kv_manager::KvManager::flatten_cache(&raw_cache);
                let out_id = active.current_app_id.clone();
                let out_model = active.model_name.clone();
                let out_fingerprint = crate::memory::Pager::get_history_fingerprint(&out_id);

                eviction_task = Some(tokio::spawn(async move {
                    crate::memory::Pager::page_out_kv_cache(
                        &out_id,
                        &out_model,
                        &flat_tensors,
                        &out_fingerprint,
                    );
                }));
            }

            // 2. APPLY THE NEW STATE
            if needs_model_reload {
                // Drop the old model, Mmap the new weights from scratch
                *state = Some(
                    Self::load_weights_into_memory(&model, app_id, stateful_paging, &self.device)
                        .map_err(|e| DriverError::ExecutionFailed(e.to_string()))?,
                );
            } else if needs_agent_swap {
                // BRAIN TRANSPLANT! Keep weights, just wipe the KV-Cache.
                let active = state.as_mut().unwrap();
                active.model.clear_kv_cache(); // Instantly frees the old agent's working memory
                active.current_app_id = app_id.to_string();
                active.stateful_paging = stateful_paging;
                active.last_used = std::time::Instant::now();
                println!(
                    "-> [NATIVE DRIVER] KV-Cache wiped for Agent Swap. Model weights retained in RAM."
                );
            } else {
                // Perfect Hit
                if let Some(active) = state.as_mut() {
                    active.last_used = std::time::Instant::now();
                    is_hot_hit = true;
                }
            }
        }

        // Wait for the SSD write to finish BEFORE doing math to prevent PCIe bandwidth traffic jams
        if let Some(task) = eviction_task {
            let _ = task.await;
        }

        let engine_arc = Arc::clone(&self.engine);
        let safe_prompt = prompt.to_string();
        let device_clone = self.device.clone();
        let history_clone = history.clone();

        let a_id = app_id.to_string();

        let curr_fingerprint = current_fingerprint.to_string();

        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut state_guard = engine_arc.lock().unwrap();
            let active = state_guard.as_mut().unwrap();

            let mut current_cache_len = 0;

            if stateful_paging {
                if is_hot_hit {
                    current_cache_len = active.model.get_kv_cache_len();
                    kprintln!("-> [NATIVE DRIVER] RAM Cache Hit ({} tokens). Bypassing SSD.", current_cache_len);
                } else if let Some(frozen_tensors) = crate::memory::Pager::page_in_kv_cache(&a_id, &model, &device_clone, &curr_fingerprint) {
                    // Unflatten the SSD file back into 3D Neural Tensors
                    let cache = crate::inference::native::kv_manager::KvManager::unflatten_cache(&frozen_tensors, active.model.num_layers());
                    // Inject directly into the Engine's brain!
                    active.model.set_kv_cache(cache);
                    current_cache_len = active.model.get_kv_cache_len();
                    kprintln!("-> [NATIVE DRIVER] KV-Cache injected ({} tokens). Bypassing Prefill.", current_cache_len);
                } else {
                    kprintln!("-> [NATIVE DRIVER] No valid KV-Cache found. Rebuilding brain from JSON history.");
                    active.model.clear_kv_cache();
                }
            } else {
                // If paging is off, ensure we start with a clean brain!
                active.model.clear_kv_cache();
            }

            let formatted_prompt = (active.config.formatter)(
                history_clone.as_deref().unwrap_or(&[]),
                &safe_prompt,
            );

            let mut tokens = active
                .tokenizer
                .encode(formatted_prompt, true)
                .unwrap()
                .get_ids()
                .to_vec();

            let mut start_pos = current_cache_len;

            // Failsafe: If the JSON history was compacted, the cache length will be larger than the new tokens!
            // We must clear the cache and do a cold start.
            if start_pos > tokens.len() {
                active.model.clear_kv_cache();
                start_pos = 0;
            }

            if start_pos > 0 {
                let cached_last_token = tokens[start_pos - 1];
                if active.config.stop_tokens.contains(&cached_last_token) {
                    // Pull start_pos back by 1 so we overwrite the Stop Token in VRAM!
                    start_pos -= 1;
                }
            }

            let overlap_start = if start_pos > 0 { start_pos - 1 } else { 0 };

            // TRUNCATE THE KV-CACHE IN VRAM TO MATCH THE OVERLAP
            if overlap_start < current_cache_len  {
                active.model.truncate_kv_cache(overlap_start);
            }

            let unprocessed_tokens = tokens[overlap_start..].to_vec();

            // Reset start_pos so the GPU loop math is perfectly aligned
            start_pos = overlap_start;

            println!("-> [NATIVE DRIVER] [DEBUG] Computing {} new tokens at position {}...", unprocessed_tokens.len(), start_pos);

            for index in 0..8192 {

                let input_len = if index == 0 { unprocessed_tokens.len() } else { 1 };

                if input_len == 0 { break; } // Edge case protection

                // Create the tensor instantly without holding a slice reference open!
                let input_tensor = if index == 0 {
                    Tensor::new(unprocessed_tokens.as_slice(), &device_clone)
                        .unwrap()
                        .unsqueeze(0)
                        .unwrap()
                } else {
                    // Dereference (*) to COPY the last token out of the array. No borrows!
                    let last_token = *tokens.last().unwrap();
                    Tensor::new(&[last_token], &device_clone)
                        .unwrap()
                        .unsqueeze(0)
                        .unwrap()
                };

                let logits = active.model.forward(&input_tensor, start_pos).unwrap();

                let logits = logits
                    .squeeze(0)
                    .unwrap()
                    .to_dtype(DType::F32)
                    .unwrap();

                let next_token_id = active.logits_processor.sample(&logits).unwrap();

                if active.config.stop_tokens.contains(&next_token_id) {
                    break;
                }

                let word = active.tokenizer.decode(&[next_token_id], true).unwrap();

                if tx.send(word).is_err() {
                    break;
                }

                tokens.push(next_token_id);

                // CRITICAL: Advance start_pos by how many tokens we just processed!
                start_pos += input_len;
            }

            drop(tx);

            active.last_used = std::time::Instant::now();

            Ok(())
        })
        .await
        .map_err(|e| DriverError::ExecutionFailed(e.to_string()))?;

        result.map_err(DriverError::ExecutionFailed)
    }

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError> {
        let mut models = Vec::new();
        let models_dir = Path::new("../models");

        if !models_dir.exists() {
            return Ok(models);
        }

        if let Ok(entries) = fs::read_dir(models_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata()
                    && metadata.is_dir()
                {
                    let folder_name = entry.file_name().to_string_lossy().to_string();
                    let gguf_path = entry.path().join("model.gguf");

                    let mut size_bytes = 0;
                    let mut modified_at = "UNKNOWN".to_string();

                    if let Ok(gguf_meta) = fs::metadata(&gguf_path) {
                        size_bytes = gguf_meta.len();

                        if let Ok(sys_time) = gguf_meta.modified() {
                            let dt: OffsetDateTime = sys_time.into();

                            let local_offset = time::UtcOffset::current_local_offset()
                                .unwrap_or(time::UtcOffset::UTC);
                            let local_dt = dt.to_offset(local_offset);

                            // Compile-time macro format! (Zero runtime parsing cost)
                            let format = format_description!(
                                "[day]-[month]-[year] [hour]:[minute]:[second]"
                            );
                            modified_at = local_dt
                                .format(&format)
                                .unwrap_or_else(|_| "UNKNOWN".to_string());
                        }
                    }

                    let display_name = folder_name.replace("-", ":");

                    models.push(LocalModel {
                        name: display_name,
                        size_bytes,
                        modified_at,
                    });
                }
            }
        }
        Ok(models)
    }

    async fn generate_embeddings(
        &self,
        model_name: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError> {
        let safe_model = model_name.replace(":", "-");
        let device = self.device.clone();

        // Spawn a blocking thread
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f32>>, String> {
            let model_dir = format!("../models/{}", safe_model);
            let config_path = format!("{}/config.json", model_dir);

            if !Path::new(&config_path).exists() {
                return Err(format!(
                    "Embedder config missing. Run 'ore pull {}'",
                    safe_model
                ));
            }

            // Detemine the architecture
            let config_str = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
            let config_val: serde_json::Value =
                serde_json::from_str(&config_str).map_err(|e| e.to_string())?;

            let arch = config_val
                .get("architectures")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("BertModel"); // Default to standard BERT if missing

            kprintln!("-> [NATIVE] Detected Embedder Architecture: '{}'", arch);

            let vectors = match arch {
                "NomicBertModel" => {
                    // Route to custom Nomic RoPE/SwiGLU implementation
                    let embedder = models::nomic::SystemEmbedder::load(&model_dir, &device)
                        .map_err(|e| format!("Failed to load Nomic embedder: {}", e))?;
                    embedder
                        .embed_batch(inputs)
                        .map_err(|e| format!("Nomic math failed: {}", e))?
                }
                "BertModel" => {
                    // Route to the ultra-fast standard MiniLM implementation
                    let embedder = models::bert::SystemEmbedder::load(&model_dir, &device)
                        .map_err(|e| format!("Failed to load BERT embedder: {}", e))?;
                    embedder
                        .embed_batch(inputs)
                        .map_err(|e| format!("BERT math failed: {}", e))?
                }
                _ => return Err(format!("Unsupported embedding architecture: {}", arch)),
            };

            // The moment this thread finishes, `embedder` goes out of scope.
            // Rust's memory safety automatically drops the model and flushes the RAM to 0MB.

            Ok(vectors)
        })
        .await
        .map_err(|e| DriverError::ExecutionFailed(e.to_string()))?;

        result.map_err(DriverError::ExecutionFailed)
    }

    // just for the sake of trait implementation, taken care by CLI
    async fn pull_model(&self, _model: &str) -> Result<(), DriverError> {
        Ok(())
    }

    async fn flush_idle_memory(&self, idle_timeout_mins: u64) -> Result<(), DriverError> {
        let mut eviction_task = None;
        {
            let mut state = self.engine.lock().unwrap();
            if let Some(active) = state.as_ref()
                && active.last_used.elapsed().as_secs() > idle_timeout_mins * 60
            {
                println!(
                    "-> [NATIVE DRIVER] Idle Timeout ({} mins) reached for '{}'. Evicting from RAM...",
                    idle_timeout_mins, active.current_app_id
                );
                if active.stateful_paging {
                    let raw_cache = active.model.get_kv_cache();
                    let flat_tensors =
                        crate::inference::native::kv_manager::KvManager::flatten_cache(&raw_cache);
                    let out_id = active.current_app_id.clone();
                    let out_model = active.model_name.clone();
                    let out_fingerprint = crate::memory::Pager::get_history_fingerprint(&out_id);

                    eviction_task = Some(tokio::spawn(async move {
                        crate::memory::Pager::page_out_kv_cache(
                            &out_id,
                            &out_model,
                            &flat_tensors,
                            &out_fingerprint,
                        );
                    }));
                }
                *state = None; // Drop the engine entirely to free RAM/VRAM
            }
        }
        if let Some(task) = eviction_task {
            let _ = task.await;
        }
        Ok(())
    }

    async fn invalidate_agent_cache(&self, app_id: &str) -> Result<(), DriverError> {
        let mut state = self.engine.lock().unwrap();

        if let Some(active) = state.as_mut()
            && active.current_app_id == app_id
        {
            println!(
                "-> [NATIVE DRIVER] Surgical Cache Invalidation: Wiping RAM KV-Cache for '{}'...",
                app_id
            );
            active.model.clear_kv_cache(); // Kill the Ghost!

            // We set the ID to something invalid so the next request is forced to do a Page-In
            // or Cold Start, rather than triggering a False "Shared Memory Hit"
            active.current_app_id = "[INVALIDATED_COMPACTION]".to_string();

            println!("-> [NATIVE DRIVER] KV-Cache wiped. Model Weights safely retained in RAM.");
        }
        Ok(())
    }
}
