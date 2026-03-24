use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

pub struct GpuScheduler {
    execution_lock: Arc<Semaphore>,
    state: Mutex<GpuState>,
}

/// Tracks what is currently physically loaded in VRAM.
struct GpuState {
    active_model: Option<String>,
    active_users: u32,
}

impl Default for GpuScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuScheduler {
    pub fn new() -> Self {
        Self {
            execution_lock: Arc::new(Semaphore::new(1)),
            state: Mutex::new(GpuState {
                active_model: None,
                active_users: 0,
            }),
        }
    }

    pub async fn request_gpu(&self, requested_model: &str) -> GpuLease {
        let permit = Arc::clone(&self.execution_lock)
            .acquire_owned()
            .await
            .unwrap();

        // 2. Check the Memory Map (What's in VRAM?)
        let mut state = self.state.lock().await;

        let is_hot_swap = state.active_model.as_ref() == Some(&requested_model.to_string());

        if is_hot_swap {
            println!(
                "-> [SCHEDULER] Shared Memory Hit! '{}' is already hot.",
                requested_model
            );
            state.active_users += 1;
        } else {
            if let Some(old) = &state.active_model {
                println!(
                    "-> [SCHEDULER] Context Switch: Evicting '{}' -> Loading '{}'",
                    old, requested_model
                );
            } else {
                println!(
                    "-> [SCHEDULER] Cold Start: Loading '{}' into VRAM.",
                    requested_model
                );
            }
            state.active_model = Some(requested_model.to_string());
            state.active_users = 1;
        }

        GpuLease {
            _permit: permit,
            model: requested_model.to_string(),
        }
    }

    /// Helper to see what's currently running
    pub async fn get_status(&self) -> String {
        let state = self.state.lock().await;
        match &state.active_model {
            Some(m) => format!("ACTIVE (Model: {}, Users: {})", m, state.active_users),
            None => "IDLE (VRAM Empty)".to_string(),
        }
    }
}

// When this struct drops (variable goes out of scope), the GPU is unlocked.
pub struct GpuLease {
    _permit: OwnedSemaphorePermit,
    pub model: String,
}
