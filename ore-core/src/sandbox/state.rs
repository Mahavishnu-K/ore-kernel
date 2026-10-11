use crate::linker::{HasLinkerState, LinkerState};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use wasmtime_wasi::p1::WasiP1Ctx;

// THE MASTER SANDBOX STATE
// Holds both the OS system calls (WASI) and the Dynamic Linker Registry
pub struct OreSandboxState {
    pub wasi: WasiP1Ctx,
    pub linker: LinkerState,
}

impl OreSandboxState {
    pub fn new(wasi: WasiP1Ctx) -> Self {
        Self {
            wasi,
            linker: LinkerState::default(),
        }
    }
}

impl HasLinkerState for OreSandboxState {
    fn linker_state(&self) -> &LinkerState {
        &self.linker
    }

    fn linker_state_mut(&mut self) -> &mut LinkerState {
        &mut self.linker
    }
}

pub struct TempDirGuard {
    pub path: PathBuf,
}

impl TempDirGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
            crate::kprintln!("-> [SANDBOX VFS] Ephemeral directory destroyed.");
        }
    }
}

pub struct ThreadJoinGuard {
    pub stop: Arc<AtomicBool>,
    pub crypto_handle: Option<JoinHandle<()>>,
    pub net_handle: Option<JoinHandle<()>>,
}

impl ThreadJoinGuard {
    pub fn new(
        stop: Arc<AtomicBool>,
        crypto_handle: Option<JoinHandle<()>>,
        net_handle: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            stop,
            crypto_handle,
            net_handle,
        }
    }
}

impl Drop for ThreadJoinGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.crypto_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.net_handle.take() {
            let _ = handle.join();
        }
    }
}
