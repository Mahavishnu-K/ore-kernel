pub mod engine;
pub mod gguf_tokenizer;
pub mod kv_manager;
pub mod models;

use crate::driver::{DriverError, EmbedTask, InferenceDriver, LocalModel, VramProcess};
use crate::memory::ContextMessage;
use crate::memory::Pager;
use anyhow::Result;
use async_trait::async_trait;
use candle_core::{DType, Device};
use engine::ActiveEngine;
use kv_manager::KvManager;
use ore_common::constants;
use ore_common::paths;
use std::fs;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use time::OffsetDateTime;
use time::macros::format_description;
use tokio::sync::mpsc::UnboundedSender;

/// A sentence embedder, picked by the `architectures` field of its config.json.
enum EmbedderModel {
    Nomic(models::nomic::SystemEmbedder),
    Bert(models::bert::SystemEmbedder),
}

impl EmbedderModel {
    fn load(model_name: &str, device: &Device) -> Result<(Self, u64), String> {
        let model_dir = paths::get_models_dir().join(model_name);
        let config_path = model_dir.join(constants::CONFIG_JSON_FILE_NAME);

        if !config_path.exists() {
            return Err(format!(
                "Embedder config missing. Run 'ore pull {}'",
                model_name
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

        let model_dir_str = model_dir.to_string_lossy().to_string();
        let model = match arch {
            // Custom Nomic RoPE/SwiGLU implementation
            "NomicBertModel" => Self::Nomic(
                models::nomic::SystemEmbedder::load(&model_dir_str, device)
                    .map_err(|e| format!("Failed to load Nomic embedder: {}", e))?,
            ),
            // Standard MiniLM implementation
            "BertModel" => Self::Bert(
                models::bert::SystemEmbedder::load(&model_dir_str, device)
                    .map_err(|e| format!("Failed to load BERT embedder: {}", e))?,
            ),
            _ => return Err(format!("Unsupported embedding architecture: {}", arch)),
        };

        let size_bytes = fs::metadata(model_dir.join(constants::MODEL_SAFETENSORS_FILE_NAME))
            .map(|m| m.len())
            .unwrap_or(0);
        Ok((model, size_bytes))
    }

    fn embed(&self, inputs: Vec<String>, task: EmbedTask) -> Result<Vec<Vec<f32>>, String> {
        match self {
            Self::Nomic(m) => m
                .embed_batch(inputs, task)
                .map_err(|e| format!("Nomic math failed: {}", e)),
            // MiniLM is symmetric: queries and documents embed the same way.
            Self::Bert(m) => m
                .embed_batch(inputs)
                .map_err(|e| format!("BERT math failed: {}", e)),
        }
    }
}

/// The embedder kept in RAM between requests. The kernel calls `unload_model` on it
/// once it has been idle for `embedder_keep_alive_secs`.
struct ResidentEmbedder {
    name: String,
    size_bytes: u64,
    model: Arc<EmbedderModel>,
    last_used: Instant,
}

pub struct NativeDriver {
    engine: Arc<StdMutex<Option<ActiveEngine>>>,
    /// Separate from `engine` so the embedder and the chat model never evict each other.
    embedder: Arc<StdMutex<Option<ResidentEmbedder>>>,
    device: Device,
}

/// Starts writing the agent's KV cache, and the token ids it holds, to SSD, stamped with
/// the agent's current history fingerprint. `None` when there is nothing to keep: the
/// agent doesn't page, or its cache is empty.
fn page_out_task(active: &ActiveEngine) -> Option<tokio::task::JoinHandle<()>> {
    if !active.stateful_paging || active.cached_tokens.is_empty() {
        return None;
    }
    let flat_tensors =
        KvManager::flatten_cache(&active.model.get_kv_cache(), &active.cached_tokens);
    let out_id = active.current_app_id.clone();
    let out_model = active.model_name.clone();
    let out_fingerprint = Pager::get_history_fingerprint(&out_id);
    Some(tokio::task::spawn_blocking(move || {
        Pager::page_out_kv_cache(&out_id, &out_model, &flat_tensors, &out_fingerprint);
    }))
}

fn same_model(a: &str, b: &str) -> bool {
    a.trim()
        .replace(':', "-")
        .eq_ignore_ascii_case(&b.trim().replace(':', "-"))
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
            embedder: Arc::new(StdMutex::new(None)),
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

    fn device_name(&self) -> String {
        match &self.device {
            Device::Cuda(i) => format!("NVIDIA GPU (CUDA): {:?}", i),
            Device::Metal(i) => format!("Apple Silicon (Metal): {:?}", i),
            Device::Cpu => "Host CPU (SIMD)".to_string(),
        }
    }

    async fn is_online(&self) -> bool {
        true
    }

    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError> {
        let placement = |weights_bytes: u64| match self.device {
            Device::Cuda(_) => (weights_bytes, 0),
            Device::Metal(_) => (weights_bytes, weights_bytes), // Unified Memory
            Device::Cpu => (0, weights_bytes),
        };
        let mut running = Vec::new();

        if let Some(active) = &*self.engine.lock().unwrap() {
            let (vram, ram) = placement(active._mmap.len() as u64);
            running.push(VramProcess {
                model_name: active.model_name.clone(),
                size_bytes: ram,
                size_vram_bytes: vram,
                status: "IDLE".to_string(),
                active_requests: 0,
                last_used_secs: active.last_used.elapsed().as_secs(),
            });
        }
        if let Some(resident) = &*self.embedder.lock().unwrap() {
            let (vram, ram) = placement(resident.size_bytes);
            running.push(VramProcess {
                model_name: resident.name.clone(),
                size_bytes: ram,
                size_vram_bytes: vram,
                status: "IDLE".to_string(),
                active_requests: 0,
                last_used_secs: resident.last_used.elapsed().as_secs(),
            });
        }
        Ok(running)
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

    /// Unloads `model` only: unloading the embedder leaves the chat model (and its
    /// KV cache) in place, and the reverse.
    async fn unload_model(&self, model: &str) -> Result<(), DriverError> {
        {
            let mut embedder = self.embedder.lock().unwrap();
            if embedder
                .as_ref()
                .is_some_and(|r| same_model(&r.name, model))
            {
                // An embedding still in flight holds its own Arc; RAM frees when it ends.
                *embedder = None;
                return Ok(());
            }
        }
        let page_out = {
            let mut state = self.engine.lock().unwrap();
            if !state
                .as_ref()
                .is_some_and(|a| same_model(&a.model_name, model))
            {
                return Ok(());
            }
            // Save a paging agent's KV cache before the weights go, so an eviction by the
            // scheduler costs an SSD page-in instead of a full prefill.
            let task = state.as_ref().and_then(page_out_task);
            *state = None;
            task
        };
        if let Some(task) = page_out {
            let _ = task.await;
        }
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
            if (needs_model_reload || needs_agent_swap)
                && let Some(active) = state.as_ref()
            {
                eviction_task = page_out_task(active);
                if eviction_task.is_some() {
                    println!(
                        "-> [NATIVE DRIVER] Lazy Eviction: Freezing Agent '{}' Brain State to SSD...",
                        active.current_app_id
                    );
                }
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
                active.reset_cache(); // Instantly frees the old agent's working memory
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

            if stateful_paging {
                if is_hot_hit {
                    kprintln!("-> [NATIVE DRIVER] RAM Cache Hit ({} tokens). Bypassing SSD.", active.cached_tokens.len());
                } else if let Some(frozen_tensors) = crate::memory::Pager::page_in_kv_cache(&a_id, &model, &device_clone, &curr_fingerprint) {
                    match KvManager::token_ids(&frozen_tensors) {
                        Some(token_ids) => {
                            // Unflatten the SSD file back into 3D Neural Tensors
                            let cache = KvManager::unflatten_cache(&frozen_tensors, active.model.num_layers());
                            active.restore_cache(cache, token_ids);
                            kprintln!("-> [NATIVE DRIVER] KV-Cache injected ({} tokens).", active.cached_tokens.len());
                        }
                        None => {
                            // Written before token ids were recorded: can't be checked, so rebuild.
                            kprintln!("-> [NATIVE DRIVER] KV-Cache on SSD has no token record. Rebuilding brain from JSON history.");
                            active.reset_cache();
                        }
                    }
                } else {
                    kprintln!("-> [NATIVE DRIVER] No valid KV-Cache found. Rebuilding brain from JSON history.");
                    active.reset_cache();
                }
            } else {
                // If paging is off, ensure we start with a clean brain!
                active.reset_cache();
            }

            let formatted_prompt = (active.config.formatter)(
                history_clone.as_deref().unwrap_or(&[]),
                &safe_prompt,
            );

            let tokens = active
                .tokenizer
                .encode(formatted_prompt, true)
                .map_err(|e| e.to_string())?
                .get_ids()
                .to_vec();

            // Reuse only the part of the cache whose tokens match this prompt exactly. The
            // cache length alone isn't enough: Auto-RAG context is in the cache but not in
            // the stored history, and a compacted history or a re-tokenized answer differs
            // from what was cached.
            let reused = active.reuse_prefix(&tokens);
            if stateful_paging {
                kprintln!(
                    "-> [NATIVE DRIVER] Reusing {} cached tokens, computing {} new.",
                    reused,
                    tokens.len() - reused
                );
            }

            let mut next_input = tokens[reused..].to_vec();
            for _ in 0..8192 {
                if next_input.is_empty() { break; } // Edge case protection

                let logits = active
                    .feed(&next_input, &device_clone)
                    .and_then(|l| Ok(l.squeeze(0)?.to_dtype(DType::F32)?))
                    .map_err(|e| e.to_string())?;

                let next_token_id = active.logits_processor.sample(&logits).map_err(|e| e.to_string())?;

                if active.config.stop_tokens.contains(&next_token_id) {
                    break;
                }

                let word = active.tokenizer.decode(&[next_token_id], true).map_err(|e| e.to_string())?;

                if tx.send(word).is_err() {
                    break;
                }

                next_input = vec![next_token_id];
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
        let models_dir = paths::get_models_dir();

        if !models_dir.exists() {
            return Ok(models);
        }

        if let Ok(entries) = fs::read_dir(models_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata()
                    && metadata.is_dir()
                {
                    let folder_name = entry.file_name().to_string_lossy().to_string();
                    if folder_name.starts_with('.') || folder_name.starts_with('_') {
                        continue;
                    }

                    let folder_path = entry.path();
                    let gguf_path = folder_path.join(constants::MODEL_GGUF_FILE_NAME);
                    let st_path = folder_path.join(constants::MODEL_SAFETENSORS_FILE_NAME);

                    let (format_label, target_file) = if gguf_path.exists() {
                        ("GGUF (Quantized)", Some(gguf_path))
                    } else if st_path.exists() {
                        ("Safetensors", Some(st_path))
                    } else {
                        // Check if directory has WASM or binary tools
                        let has_binary = fs::read_dir(&folder_path)
                            .map(|r| {
                                r.flatten().any(|e| {
                                    let n = e.file_name().to_string_lossy().to_string();
                                    n.ends_with(".wasm") || n.ends_with(".bin")
                                })
                            })
                            .unwrap_or(false);

                        if has_binary {
                            ("WASM Cartridge", None)
                        } else {
                            // Empty or uninitialized directory - skip
                            continue;
                        }
                    };

                    let mut size_bytes = 0;
                    let mut modified_at = "UNKNOWN".to_string();
                    let mut latest_time: Option<std::time::SystemTime> = None;

                    if let Ok(files) = fs::read_dir(&folder_path) {
                        for f in files.flatten() {
                            if let Ok(m) = f.metadata()
                                && m.is_file()
                            {
                                size_bytes += m.len();
                                if let Ok(mod_time) = m.modified() {
                                    latest_time = match latest_time {
                                        Some(prev) => Some(prev.max(mod_time)),
                                        None => Some(mod_time),
                                    };
                                }
                            }
                        }
                    }

                    if let Some(target) = target_file
                        && let Ok(fmeta) = fs::metadata(&target)
                        && let Ok(st) = fmeta.modified()
                    {
                        latest_time = Some(st);
                    }

                    if let Some(sys_time) = latest_time {
                        let dt: OffsetDateTime = sys_time.into();
                        let local_offset =
                            time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
                        let local_dt = dt.to_offset(local_offset);

                        let format =
                            format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
                        modified_at = local_dt
                            .format(&format)
                            .unwrap_or_else(|_| "UNKNOWN".to_string());
                    }

                    let display_name =
                        if folder_name == "all-minilm" || folder_name == "system-embedder" {
                            folder_name
                        } else if let Some(idx) = folder_name.rfind('-') {
                            let suffix = &folder_name[idx + 1..];
                            if suffix.ends_with('b')
                                || suffix.starts_with('q')
                                || suffix.parse::<f64>().is_ok()
                            {
                                format!("{}:{}", &folder_name[..idx], suffix)
                            } else {
                                folder_name
                            }
                        } else {
                            folder_name
                        };

                    models.push(LocalModel {
                        name: display_name,
                        size_bytes,
                        modified_at,
                        format: format_label.to_string(),
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
        task: EmbedTask,
    ) -> Result<Vec<Vec<f32>>, DriverError> {
        let safe_model = model_name.replace(":", "-");
        let device = self.device.clone();
        let slot = Arc::clone(&self.embedder);

        // Spawn a blocking thread
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f32>>, String> {
            let resident = slot
                .lock()
                .unwrap()
                .as_ref()
                .filter(|r| r.name == safe_model)
                .map(|r| Arc::clone(&r.model));

            let model = match resident {
                Some(model) => model,
                None => {
                    // Loaded outside the slot lock, so /ps and /health never wait on disk I/O.
                    let (model, size_bytes) = EmbedderModel::load(&safe_model, &device)?;
                    let model = Arc::new(model);
                    *slot.lock().unwrap() = Some(ResidentEmbedder {
                        name: safe_model.clone(),
                        size_bytes,
                        model: Arc::clone(&model),
                        last_used: Instant::now(),
                    });
                    model
                }
            };

            let vectors = model.embed(inputs, task)?;

            if let Some(r) = slot.lock().unwrap().as_mut()
                && r.name == safe_model
            {
                r.last_used = Instant::now();
            }
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
                eviction_task = page_out_task(active);
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
            active.reset_cache();

            // We set the ID to something invalid so the next request is forced to do a Page-In
            // or Cold Start, rather than triggering a False "Shared Memory Hit"
            active.current_app_id = "[INVALIDATED_COMPACTION]".to_string();

            println!("-> [NATIVE DRIVER] KV-Cache wiped. Model Weights safely retained in RAM.");
        }
        Ok(())
    }

    async fn count_tokens(&self, model: &str, text: &str) -> Option<u32> {
        let safe_name = model.trim().replace(':', "-");

        // In-memory check: if the model is currently active, use its loaded tokenizer directly
        {
            let state = self.engine.lock().unwrap();
            if let Some(active) = &*state
                && (active.model_name == safe_name || active.model_name == model)
                && let Ok(encoding) = active.tokenizer.encode(text, false)
            {
                return Some(encoding.get_ids().len() as u32);
            }
        }

        // On-disk check: read cached tokenizer.json if available
        let model_dir = paths::get_models_dir().join(&safe_name);
        let tokenizer_path = model_dir.join(constants::TOKENIZER_JSON_FILE_NAME);
        if tokenizer_path.exists()
            && let Ok(tok) = tokenizers::Tokenizer::from_file(&tokenizer_path)
            && let Ok(encoding) = tok.encode(text, false)
        {
            return Some(encoding.get_ids().len() as u32);
        }

        None
    }
}
