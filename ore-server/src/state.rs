use ore_core::driver::{DriverError, EmbedTask, InferenceDriver};
use ore_core::ipc::{MessageBus, RateLimiter, SemanticBus};
use ore_core::registry::AppRegistry;
use ore_core::sandbox::WasmSandbox;
use ore_core::scheduler::GpuScheduler;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub use ore_common::config::*;

/// RAII Drop guard ensuring embedding weights are evicted from memory
/// even if a panic or unexpected early exit occurs.
pub struct ScopedEmbedderLease {
    driver: Arc<dyn InferenceDriver>,
    model_name: String,
}

impl ScopedEmbedderLease {
    pub fn new(driver: Arc<dyn InferenceDriver>, model_name: String) -> Self {
        Self { driver, model_name }
    }
}

impl Drop for ScopedEmbedderLease {
    fn drop(&mut self) {
        let driver = Arc::clone(&self.driver);
        let model = self.model_name.clone();
        tokio::spawn(async move {
            let _ = driver.unload_model(&model).await;
            ore_core::kprintln!(
                "-> [MMU LEASE] System Embedder safely evicted from memory (0MB Idle)."
            );
        });
    }
}

// kernel state shared across handlers
pub struct KernelState {
    pub driver: Arc<dyn InferenceDriver>,
    pub scheduler: Arc<GpuScheduler>,
    pub embedder_lock: Arc<Mutex<()>>,
    pub registry: AppRegistry,
    pub semantic_bus: Arc<SemanticBus>,
    pub message_bus: MessageBus,
    pub rate_limiter: RateLimiter,
    pub auth_token: String,
    pub system_embedder: String,
    /// How long the embedder stays loaded after its last request (0 = unload at once).
    pub embedder_keep_alive: Duration,
    /// When the embedder last finished a request; `None` while it is not loaded.
    pub embedder_last_used: StdMutex<Option<Instant>>,
    pub sandbox: Arc<WasmSandbox>,
}

impl KernelState {
    /// Embeds `inputs` with the system embedder, one request at a time.
    ///
    /// The embedder stays loaded for `embedder_keep_alive` after the request, so a burst
    /// of shares and searches loads it once; `evict_idle_embedder` unloads it afterwards.
    /// With a keep-alive of 0 it is unloaded before the lock is released.
    pub async fn generate_embeddings_scoped(
        &self,
        inputs: Vec<String>,
        task: EmbedTask,
    ) -> Result<Vec<Vec<f32>>, DriverError> {
        let _guard = self.embedder_lock.lock().await;
        let res = self
            .driver
            .generate_embeddings(&self.system_embedder, inputs, task)
            .await;
        if self.embedder_keep_alive.is_zero() {
            let _ = self.driver.unload_model(&self.system_embedder).await;
        } else {
            *self.embedder_last_used.lock().unwrap() = Some(Instant::now());
        }
        res
    }

    /// Unloads the embedder once it has been idle for `embedder_keep_alive`.
    /// Called periodically by the kernel's background loop.
    pub async fn evict_idle_embedder(&self) {
        if self.embedder_keep_alive.is_zero() {
            return;
        }
        // A request in flight holds the lock: the embedder is in use, not idle.
        let Ok(_guard) = self.embedder_lock.try_lock() else {
            return;
        };
        let expired = {
            let mut last_used = self.embedder_last_used.lock().unwrap();
            match *last_used {
                Some(t) if t.elapsed() >= self.embedder_keep_alive => {
                    *last_used = None;
                    true
                }
                _ => false,
            }
        };
        if expired {
            let _ = self.driver.unload_model(&self.system_embedder).await;
            ore_core::kprintln!(
                "-> [MMU] System Embedder idle for {}s. Evicted from memory.",
                self.embedder_keep_alive.as_secs()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ore_core::driver::{LocalModel, VramProcess};
    use ore_core::memory::ContextMessage;
    use tokio::sync::mpsc::UnboundedSender;

    /// Records embed/unload calls instead of running a model.
    #[derive(Default)]
    struct FakeDriver {
        calls: StdMutex<Vec<String>>,
    }

    impl FakeDriver {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl InferenceDriver for FakeDriver {
        fn engine_name(&self) -> &'static str {
            "fake"
        }
        async fn is_online(&self) -> bool {
            true
        }
        async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError> {
            Ok(vec![])
        }
        async fn unload_model(&self, model: &str) -> Result<(), DriverError> {
            self.calls.lock().unwrap().push(format!("unload {model}"));
            Ok(())
        }
        async fn preload_model(&self, _: &str) -> Result<(), DriverError> {
            Ok(())
        }
        async fn pull_model(&self, _: &str) -> Result<(), DriverError> {
            Ok(())
        }
        async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError> {
            Ok(vec![])
        }
        async fn generate_text(
            &self,
            _: &str,
            _: &str,
            _: bool,
            _: &str,
            _: Option<Vec<ContextMessage>>,
            _: UnboundedSender<String>,
            _: &str,
        ) -> Result<(), DriverError> {
            Ok(())
        }
        async fn generate_embeddings(
            &self,
            model: &str,
            inputs: Vec<String>,
            task: EmbedTask,
        ) -> Result<Vec<Vec<f32>>, DriverError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("embed {model} {task:?}"));
            Ok(inputs.iter().map(|_| vec![1.0]).collect())
        }
        async fn flush_idle_memory(&self, _: u64) -> Result<(), DriverError> {
            Ok(())
        }
        async fn invalidate_agent_cache(&self, _: &str) -> Result<(), DriverError> {
            Ok(())
        }
    }

    fn kernel(driver: Arc<FakeDriver>, keep_alive: Duration) -> KernelState {
        let manifests = std::env::temp_dir().join(format!("ore-state-test-{}", std::process::id()));
        std::fs::create_dir_all(&manifests).unwrap();
        let driver: Arc<dyn InferenceDriver> = driver;
        KernelState {
            scheduler: Arc::new(GpuScheduler::new(
                Arc::clone(&driver),
                Box::new(ore_core::scheduler::SystemMemoryProvider::new()),
                ore_core::scheduler::SchedulerConfig::default(),
            )),
            driver,
            embedder_lock: Arc::new(Mutex::new(())),
            registry: AppRegistry::boot_load(manifests.to_str().unwrap()).unwrap(),
            semantic_bus: Arc::new(SemanticBus::new(24, 24)),
            message_bus: MessageBus::new(),
            rate_limiter: RateLimiter::new(),
            auth_token: String::new(),
            system_embedder: "system-embedder".to_string(),
            embedder_keep_alive: keep_alive,
            embedder_last_used: StdMutex::new(None),
            sandbox: Arc::new(WasmSandbox::new().unwrap()),
        }
    }

    #[tokio::test]
    async fn embedder_stays_loaded_until_idle_for_keep_alive() {
        let driver = Arc::new(FakeDriver::default());
        let state = kernel(Arc::clone(&driver), Duration::from_millis(150));

        state
            .generate_embeddings_scoped(vec!["a".into()], EmbedTask::Document)
            .await
            .unwrap();
        state
            .generate_embeddings_scoped(vec!["b".into()], EmbedTask::Query)
            .await
            .unwrap();
        state.evict_idle_embedder().await; // used just now: stays
        assert_eq!(
            driver.calls(),
            [
                "embed system-embedder Document",
                "embed system-embedder Query"
            ]
        );

        tokio::time::sleep(Duration::from_millis(200)).await;
        state.evict_idle_embedder().await;
        state.evict_idle_embedder().await; // already unloaded: no second unload
        assert_eq!(driver.calls().last().unwrap(), "unload system-embedder");
        assert_eq!(driver.calls().len(), 3);
    }

    #[tokio::test]
    async fn busy_embedder_is_never_evicted() {
        let driver = Arc::new(FakeDriver::default());
        let state = kernel(Arc::clone(&driver), Duration::from_millis(10));
        state
            .generate_embeddings_scoped(vec!["a".into()], EmbedTask::Document)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;

        let in_flight = state.embedder_lock.lock().await; // a request is running
        state.evict_idle_embedder().await;
        assert!(!driver.calls().iter().any(|c| c.starts_with("unload")));
        drop(in_flight);

        state.evict_idle_embedder().await;
        assert_eq!(driver.calls().last().unwrap(), "unload system-embedder");
    }

    #[tokio::test]
    async fn zero_keep_alive_unloads_after_every_request() {
        let driver = Arc::new(FakeDriver::default());
        let state = kernel(Arc::clone(&driver), Duration::ZERO);
        state
            .generate_embeddings_scoped(vec!["a".into()], EmbedTask::Document)
            .await
            .unwrap();
        assert_eq!(
            driver.calls(),
            ["embed system-embedder Document", "unload system-embedder"]
        );
    }
}
