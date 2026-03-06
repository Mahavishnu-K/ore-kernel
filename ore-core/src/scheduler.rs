use tokio::sync::{Mutex, Semaphore, SemaphorePermit};

pub struct GpuScheduler {
    /// The physical lock. Only 1 process can do heavy compute at a time.
    execution_lock: Semaphore,
    
    /// We use a Mutex because multiple threads need to read/update this state.
    state: Mutex<GpuState>,
}

/// Tracks what is currently physically loaded in VRAM.
struct GpuState {
    active_model: Option<String>,
    active_users: u32,
}

impl GpuScheduler {
    pub fn new() -> Self {
        Self {
            execution_lock: Semaphore::new(1),
            state: Mutex::new(GpuState {
                active_model: None,
                active_users: 0,
            }),
        }
    }

    pub async fn request_gpu(&self, requested_model: &str) -> GpuLease<'_> {
        
        let permit = self.execution_lock.acquire().await.unwrap();
        
        // 2. Check the Memory Map (What's in VRAM?)
        let mut state = self.state.lock().await;
        
        let is_hot_swap = if let Some(current) = &state.active_model {
            current == requested_model
        } else {
            false
        };

        if is_hot_swap {
            println!("-> [SCHEDULER] Shared Memory Hit! '{}' is already hot.", requested_model);
            state.active_users += 1;
        } else {
            if let Some(old) = &state.active_model {
                println!("-> [SCHEDULER] Context Switch: Evicting '{}' -> Loading '{}'", old, requested_model);
            } else {
                println!("-> [SCHEDULER] Cold Start: Loading '{}' into VRAM.", requested_model);
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
pub struct GpuLease<'a> {
    _permit: SemaphorePermit<'a>,
    pub model: String,
}