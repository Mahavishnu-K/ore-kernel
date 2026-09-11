use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use std::time::Instant;

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

pub struct MockGpuMemoryProvider {
    pub total_vram: u64,
    pub used_vram: u64,
}

impl Default for MockGpuMemoryProvider {
    fn default() -> Self {
        Self {
            total_vram: 24576, // 24GB default mock
            used_vram: 1024,   // 1GB base usage mock
        }
    }
}

impl GpuMemoryProvider for MockGpuMemoryProvider {
    fn total_vram_mb(&self) -> u64 {
        self.total_vram
    }
    fn used_vram_mb(&self) -> u64 {
        self.used_vram
    }
    fn free_vram_mb(&self) -> u64 {
        self.total_vram.saturating_sub(self.used_vram)
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
        if let Ok(device) = self.nvml.device_by_index(self.device_index) {
            if let Ok(info) = device.memory_info() {
                return info.total / (1024 * 1024);
            }
        }
        0
    }
    fn used_vram_mb(&self) -> u64 {
        if let Ok(device) = self.nvml.device_by_index(self.device_index) {
            if let Ok(info) = device.memory_info() {
                return info.used / (1024 * 1024);
            }
        }
        0
    }
    fn free_vram_mb(&self) -> u64 {
        if let Ok(device) = self.nvml.device_by_index(self.device_index) {
            if let Ok(info) = device.memory_info() {
                return info.free / (1024 * 1024);
            }
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
        let available = memory_provider.free_vram_mb().saturating_sub(self.reserved_vram_mb);
        available >= required_vram_mb + self.safety_margin_mb
    }

    pub fn reserve(&mut self, amount_mb: u64) {
        self.reserved_vram_mb += amount_mb;
    }

    pub fn release_reservation(&mut self, amount_mb: u64) {
        self.reserved_vram_mb = self.reserved_vram_mb.saturating_sub(amount_mb);
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

pub struct SchedulerConfig {
    pub model_overrides: HashMap<String, u64>, // KV cache MB per 1k context
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            model_overrides: HashMap::new(),
        }
    }
}

struct GpuState {
    registry: ModelRegistry,
    memory_provider: Box<dyn GpuMemoryProvider>,
    accountant: MemoryAccountant,
}

pub struct GpuScheduler {
    execution_lock: Arc<Semaphore>,
    state: Arc<Mutex<GpuState>>,
    driver: Arc<dyn crate::driver::InferenceDriver>,
    config: SchedulerConfig,
}

impl GpuScheduler {
    pub fn new(driver: Arc<dyn crate::driver::InferenceDriver>, memory_provider: Box<dyn GpuMemoryProvider>, config: SchedulerConfig) -> Self {
        Self {
            execution_lock: Arc::new(Semaphore::new(1)),
            state: Arc::new(Mutex::new(GpuState {
                registry: ModelRegistry::new(),
                memory_provider,
                accountant: MemoryAccountant::new(),
            })),
            driver,
            config,
        }
    }

    pub async fn request_gpu(&self, requested_model: &str, app_id: &str) -> Result<GpuLease, String> {
        // Keep the global execution lock as per the plan
        let permit = Arc::clone(&self.execution_lock)
            .acquire_owned()
            .await
            .unwrap();

        let mut state = self.state.lock().await;

        let mut required_kv_cache_mb = 256; // Fallback heuristic
        if let Some(override_mb) = self.config.model_overrides.get(requested_model) {
            required_kv_cache_mb = *override_mb;
        }
        
        let mut required_vram_mb = required_kv_cache_mb;
        let is_cold_start = !state.registry.models.contains_key(requested_model);

        let mut estimated_model_mb = 2048; // Fallback

        if is_cold_start {
            // Check driver for model sizes
            if let Ok(models) = self.driver.get_running_models().await {
                if let Some(vram_process) = models.iter().find(|m| m.model_name == requested_model) {
                    estimated_model_mb = vram_process.size_vram_bytes / (1024 * 1024);
                }
            }
            required_vram_mb += estimated_model_mb;
        }

        // Admission Control & LRU Eviction
        while !state.accountant.can_admit(&*state.memory_provider, required_vram_mb) {
            let mut lru_model_id: Option<String> = None;
            let mut oldest_time = Instant::now();

            for (id, model) in &state.registry.models {
                if model.active_requests == 0 && model.status == ModelStatus::Loaded {
                    if model.last_used <= oldest_time {
                        oldest_time = model.last_used;
                        lru_model_id = Some(id.clone());
                    }
                }
            }

            if let Some(id) = lru_model_id {
                println!("-> [SCHEDULER] Memory Pressure: Evicting idle model '{}'.", id);
                if let Err(e) = self.driver.unload_model(&id).await {
                    println!("-> [SCHEDULER] WARNING: Failed to unload model '{}': {}", id, e);
                }
                state.registry.models.remove(&id);
            } else {
                return Err(format!(
                    "Out of VRAM budget. Required: {}MB. Cannot admit request for model: {}",
                    required_vram_mb, requested_model
                ));
            }
        }

        // Reserve memory for the incoming request
        state.accountant.reserve(required_kv_cache_mb);

        if is_cold_start {
            println!(
                "-> [SCHEDULER] Cold Start: Loading '{}' into VRAM for '{}'.",
                requested_model, app_id
            );
            
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
            
            state.registry.models.insert(requested_model.to_string(), new_model);
        } else {
            println!(
                "-> [SCHEDULER] Hot Hit! '{}' is already loaded for Agent '{}'.",
                requested_model, app_id
            );
            let model = state.registry.models.get_mut(requested_model).unwrap();
            model.active_requests += 1;
            model.last_used = Instant::now();
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
        let mut state = self.state.lock().await;
        let actual_used = state.memory_provider.used_vram_mb();
        
        let accounted: u64 = state.registry.models.values()
            .filter(|m| m.status == ModelStatus::Loaded)
            .map(|m| m.observed_load_delta_mb.unwrap_or(m.estimated_vram_mb))
            .sum::<u64>() + state.accountant.reserved_vram_mb;

        println!("-> [SCHEDULER] Reconciliation: Actual VRAM Used: {}MB | Accounted: {}MB", actual_used, accounted);

        let drift = (actual_used as i64) - (accounted as i64);

        if drift.abs() > 1024 { // 1GB drift warning
            println!("-> [SCHEDULER] WARNING: Large VRAM drift detected ({}MB).", drift);
        }
    }

    /// Handle sudden backend/CUDA OOM
    pub async fn handle_oom(&self) {
        let mut state = self.state.lock().await;
        println!("-> [SCHEDULER] ALERT: CUDA OOM detected! Reconciling state...");
        // Re-check real memory, drop failed models, maybe reclaim idle ones immediately
        state.registry.models.retain(|_, model| model.status == ModelStatus::Loaded);
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
