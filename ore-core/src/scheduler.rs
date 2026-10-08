use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;
use sysinfo::System;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStatus {
    Loading,
    Loaded,
    Unloading,
    Failed,
}

#[derive(Debug, Clone)]
pub struct LoadedModel {
    pub model_id: String,
    pub estimated_vram_mb: u64,
    pub observed_load_delta_mb: Option<u64>,
    pub active_requests: usize,
    pub last_used: Instant,
    pub status: ModelStatus,
}

pub trait GpuMemoryProvider: Send + Sync {
    fn total_vram_mb(&self) -> u64;
    fn used_vram_mb(&self) -> u64;
    fn free_vram_mb(&self) -> u64;
}

// THE UNIVERSAL FALLBACK (For Apple Metal, Intel, AMD, and CPU-only Cloud Servers)
pub struct SystemMemoryProvider {
    sys: StdMutex<System>,
}

impl SystemMemoryProvider {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_memory();
        Self {
            sys: StdMutex::new(sys),
        }
    }
}

impl Default for SystemMemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuMemoryProvider for SystemMemoryProvider {
    fn total_vram_mb(&self) -> u64 {
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_memory();
        sys.total_memory() / (1024 * 1024)
    }
    fn used_vram_mb(&self) -> u64 {
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_memory();
        sys.used_memory() / (1024 * 1024)
    }
    fn free_vram_mb(&self) -> u64 {
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_memory();
        sys.available_memory() / (1024 * 1024)
    }
}

pub struct NvmlGpuMemoryProvider {
    nvml: nvml_wrapper::Nvml,
    device_index: u32,
}

impl NvmlGpuMemoryProvider {
    pub fn new(device_index: u32) -> Result<Self, String> {
        let nvml = nvml_wrapper::Nvml::init().map_err(|e| format!("NVML init failed: {}", e))?;
        Ok(Self { nvml, device_index })
    }
}

impl GpuMemoryProvider for NvmlGpuMemoryProvider {
    fn total_vram_mb(&self) -> u64 {
        if let Ok(device) = self.nvml.device_by_index(self.device_index)
            && let Ok(info) = device.memory_info()
        {
            return info.total / (1024 * 1024);
        }
        0
    }
    fn used_vram_mb(&self) -> u64 {
        if let Ok(device) = self.nvml.device_by_index(self.device_index)
            && let Ok(info) = device.memory_info()
        {
            return info.used / (1024 * 1024);
        }
        0
    }
    fn free_vram_mb(&self) -> u64 {
        if let Ok(device) = self.nvml.device_by_index(self.device_index)
            && let Ok(info) = device.memory_info()
        {
            return info.free / (1024 * 1024);
        }
        0
    }
}

pub struct MemoryAccountant {
    pub reserved_vram_mb: u64,
    pub safety_margin_mb: u64,
}

impl MemoryAccountant {
    pub fn new() -> Self {
        Self {
            reserved_vram_mb: 0,
            safety_margin_mb: 512, // 512MB safety buffer
        }
    }

    pub fn can_admit(
        &self,
        memory_provider: &dyn GpuMemoryProvider,
        required_vram_mb: u64,
    ) -> bool {
        let available = memory_provider
            .free_vram_mb()
            .saturating_sub(self.reserved_vram_mb);
        available >= required_vram_mb + self.safety_margin_mb
    }

    pub fn reserve(&mut self, amount_mb: u64) {
        self.reserved_vram_mb += amount_mb;
    }

    pub fn release_reservation(&mut self, amount_mb: u64) {
        self.reserved_vram_mb = self.reserved_vram_mb.saturating_sub(amount_mb);
    }
}

impl Default for MemoryAccountant {
    fn default() -> Self {
        Self::new()
    }
}
pub struct ModelRegistry {
    pub models: HashMap<String, LoadedModel>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
        }
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
pub struct SchedulerConfig {
    pub model_overrides: HashMap<String, u64>, // KV cache MB per 1k context
}

struct GpuState {
    registry: ModelRegistry,
    memory_provider: Box<dyn GpuMemoryProvider>,
    accountant: MemoryAccountant,
    active_app_id: Option<String>,
}

pub struct GpuScheduler {
    execution_lock: Arc<Semaphore>,
    state: Arc<Mutex<GpuState>>,
    driver: Arc<dyn crate::driver::InferenceDriver>,
    config: SchedulerConfig,
}

impl GpuScheduler {
    pub fn new(
        driver: Arc<dyn crate::driver::InferenceDriver>,
        memory_provider: Box<dyn GpuMemoryProvider>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            execution_lock: Arc::new(Semaphore::new(32)),
            state: Arc::new(Mutex::new(GpuState {
                registry: ModelRegistry::new(),
                memory_provider,
                accountant: MemoryAccountant::new(),
                active_app_id: None,
            })),
            driver,
            config,
        }
    }

    pub async fn request_gpu(
        &self,
        requested_model: &str,
        app_id: &str,
    ) -> Result<GpuLease, String> {
        // Keep the global execution lock as per the plan
        let permit = Arc::clone(&self.execution_lock)
            .acquire_owned()
            .await
            .unwrap();

        let model_path = crate::get_ore_dir()
            .join("models")
            .join(requested_model)
            .join("model.gguf");

        let mut estimated_model_mb = 2048;
        let mut required_kv_cache_mb = 256;

        if let Ok(mut file) = std::fs::File::open(&model_path) {
            // 1. EXACT MODEL WEIGHTS SIZE
            if let Ok(meta) = file.metadata() {
                estimated_model_mb = meta.len() / (1024 * 1024);
            }

            // 2. EXACT KV-CACHE SIZE CALCULATION
            if let Ok(content) = candle_core::quantized::gguf_file::Content::read(&mut file) {
                let md_get_u32 = |s: &str| -> Option<u32> {
                    content.metadata.get(s).and_then(|v| v.to_u32().ok())
                };

                let arch = content
                    .metadata
                    .get("general.architecture")
                    .and_then(|v| v.to_string().ok())
                    .cloned()
                    .unwrap_or_else(|| "llama".to_string());

                // Extract the physical network dimensions
                let layers = md_get_u32(&format!("{}.block_count", arch)).unwrap_or(32) as u64;
                let kv_heads =
                    md_get_u32(&format!("{}.attention.head_count_kv", arch)).unwrap_or(8) as u64;

                let head_dim =
                    if let Some(hd) = md_get_u32(&format!("{}.attention.key_length", arch)) {
                        hd as u64
                    } else {
                        let heads = md_get_u32(&format!("{}.attention.head_count", arch))
                            .unwrap_or(32) as u64;
                        let emb_len = md_get_u32(&format!("{}.embedding_length", arch))
                            .unwrap_or(4096) as u64;
                        emb_len / heads
                    };

                // Bytes per Token = 2 (K & V) * Layers * KV_Heads * Head_Dim * 2 (f16 bytes)
                let bytes_per_token = 2 * layers * kv_heads * head_dim * 2;

                // Multiply by the Manifest's Max Token Limit!
                let max_tokens = 8192; // Default limit fallback
                let total_kv_bytes = bytes_per_token * max_tokens;

                required_kv_cache_mb = (total_kv_bytes / (1024 * 1024)).max(1); // Ensure at least 1MB

                crate::kprintln!(
                    "-> [SCHEDULER MATH] Model: {} | Layers: {} | KV Heads: {} | Head Dim: {} | Max Tokens: {}",
                    requested_model,
                    layers,
                    kv_heads,
                    head_dim,
                    max_tokens
                );
                crate::kprintln!(
                    "-> [SCHEDULER MATH] Exact KV-Cache Requirement: {} MB",
                    required_kv_cache_mb
                );
            }
        }

        if let Some(override_mb) = self.config.model_overrides.get(requested_model) {
            required_kv_cache_mb = *override_mb;
        }

        let mut required_vram_mb = required_kv_cache_mb;

        let is_same_model = {
            let state = self.state.lock().await;
            state.registry.models.contains_key(requested_model)
        };

        if !is_same_model {
            required_vram_mb += estimated_model_mb;
        }

        // Admission Control & LRU Eviction Loop
        loop {
            let lru_model_id = {
                let state = self.state.lock().await;
                if state
                    .accountant
                    .can_admit(&*state.memory_provider, required_vram_mb)
                {
                    break;
                }

                let mut oldest_id = None;
                let mut oldest_time = Instant::now();

                for (id, model) in &state.registry.models {
                    if model.active_requests == 0
                        && model.status == ModelStatus::Loaded
                        && model.last_used <= oldest_time
                    {
                        oldest_time = model.last_used;
                        oldest_id = Some(id.clone());
                    }
                }
                oldest_id
            };

            if let Some(id) = lru_model_id {
                println!(
                    "-> [SCHEDULER] Memory Pressure: Evicting idle model '{}'.",
                    id
                );
                // Lock is dropped! Safe to do slow I/O.
                if let Err(e) = self.driver.unload_model(&id).await {
                    println!(
                        "-> [SCHEDULER] WARNING: Failed to unload model '{}': {}",
                        id, e
                    );
                }

                let mut state = self.state.lock().await;
                state.registry.models.remove(&id);
            } else {
                return Err(format!(
                    "Out of VRAM budget. Required: {}MB. Cannot admit request for model: {}",
                    required_vram_mb, requested_model
                ));
            }
        }

        // Reacquire state lock for final operations
        let mut state = self.state.lock().await;

        let is_same_model = state.registry.models.contains_key(requested_model);
        let is_same_agent = state.active_app_id.as_deref() == Some(app_id);

        state.accountant.reserve(required_kv_cache_mb);

        if is_same_model && is_same_agent {
            println!(
                "-> [SCHEDULER] Perfect Hit! '{}' is already loaded for Agent '{}'.",
                requested_model, app_id
            );
            let model = state.registry.models.get_mut(requested_model).unwrap();
            model.active_requests += 1;
            model.last_used = Instant::now();
        } else if is_same_model && !is_same_agent {
            println!(
                "-> [SCHEDULER] TIER 2: AGENT SWAP! Keep weights, swap KV-Cache for '{}'.",
                app_id
            );
            state.active_app_id = Some(app_id.to_string());
            let model = state.registry.models.get_mut(requested_model).unwrap();
            model.active_requests += 1;
            model.last_used = Instant::now();
        } else {
            println!(
                "-> [SCHEDULER] TIER 3: COLD START. Loading '{}' into VRAM for '{}'.",
                requested_model, app_id
            );
            state.active_app_id = Some(app_id.to_string());

            // Preload the model
            if let Err(e) = self.driver.preload_model(requested_model).await {
                // Release reservation on failure
                state.accountant.release_reservation(required_kv_cache_mb);
                return Err(format!("Failed to load model '{}': {}", requested_model, e));
            }

            let new_model = LoadedModel {
                model_id: requested_model.to_string(),
                estimated_vram_mb: estimated_model_mb,
                observed_load_delta_mb: None,
                active_requests: 1,
                last_used: Instant::now(),
                status: ModelStatus::Loaded,
            };

            state
                .registry
                .models
                .insert(requested_model.to_string(), new_model);
        }

        Ok(GpuLease {
            _permit: permit,
            model: requested_model.to_string(),
            reserved_kv_cache_mb: required_kv_cache_mb,
            state: Arc::clone(&self.state),
        })
    }

    /// Reconcile logical memory accounting with physical GPU memory
    pub async fn reconcile_memory(&self) {
        let state = self.state.lock().await;
        let actual_used = state.memory_provider.used_vram_mb();

        let accounted: u64 = state
            .registry
            .models
            .values()
            .filter(|m| m.status == ModelStatus::Loaded)
            .map(|m| m.observed_load_delta_mb.unwrap_or(m.estimated_vram_mb))
            .sum::<u64>()
            + state.accountant.reserved_vram_mb;

        println!(
            "-> [SCHEDULER] Reconciliation: Actual VRAM Used: {}MB | Accounted: {}MB",
            actual_used, accounted
        );

        let drift = (actual_used as i64) - (accounted as i64);

        if drift.abs() > 1024 {
            // 1GB drift warning
            println!(
                "-> [SCHEDULER] WARNING: Large VRAM drift detected ({}MB).",
                drift
            );
        }
    }

    /// Handle sudden backend/CUDA OOM
    pub async fn handle_oom(&self) {
        let mut state = self.state.lock().await;
        println!("-> [SCHEDULER] ALERT: CUDA OOM detected! Reconciling state...");
        // Re-check real memory, drop failed models, maybe reclaim idle ones immediately
        state
            .registry
            .models
            .retain(|_, model| model.status == ModelStatus::Loaded);
    }

    pub async fn get_status(&self) -> String {
        let state = self.state.lock().await;

        let mut status = format!(
            "VRAM Free: {}MB, Reserved: {}MB\n",
            state.memory_provider.free_vram_mb(),
            state.accountant.reserved_vram_mb
        );

        if state.registry.models.is_empty() {
            status.push_str("IDLE (No models loaded)");
        } else {
            status.push_str("ACTIVE:\n");
            for (id, model) in &state.registry.models {
                status.push_str(&format!(
                    " - Model: {}, Status: {:?}, Active Requests: {}\n",
                    id, model.status, model.active_requests
                ));
            }
        }
        status
    }
}

pub struct GpuLease {
    _permit: OwnedSemaphorePermit,
    pub model: String,
    reserved_kv_cache_mb: u64,
    state: Arc<Mutex<GpuState>>,
}

impl Drop for GpuLease {
    fn drop(&mut self) {
        let model_id = self.model.clone();
        let state = Arc::clone(&self.state);
        let reserved_kv_cache = self.reserved_kv_cache_mb;

        // Decrement active requests and release reservation when the lease is dropped
        tokio::spawn(async move {
            let mut state = state.lock().await;
            if let Some(model) = state.registry.models.get_mut(&model_id) {
                model.active_requests = model.active_requests.saturating_sub(1);
            }
            state.accountant.release_reservation(reserved_kv_cache);
        });
    }
}
