use crate::crypto::KernelCrypto;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub fn spawn_crypto_portal(
    host_tmp_dir: &Path,
    stop_signal: Arc<AtomicBool>,
) -> Result<JoinHandle<()>> {
    let crypto_dir = host_tmp_dir.join(".ore_crypto");
    std::fs::create_dir_all(&crypto_dir)?;

    let portal_req = crypto_dir.join("req.bin");
    let portal_res = crypto_dir.join("res.bin");
    let portal_tmp_res = crypto_dir.join("res.tmp");

    // Pre-create the files to prevent O_CREAT ENOTSUP issues in QuickJS on Windows
    let _ = std::fs::write(&portal_req, b"");
    let _ = std::fs::write(&portal_res, b"");

    let watcher_req = portal_req;
    let watcher_res = portal_res;
    let watcher_tmp_res = portal_tmp_res;
    let watcher_stop = stop_signal;

    let handle = thread::spawn(move || {
        crate::kprintln!(
            "-> [SANDBOX] Crypto Portal Thread started. Polling for requests in {}...",
            watcher_req.display()
        );
        while !watcher_stop.load(Ordering::Relaxed) {
            if let Ok(meta) = std::fs::metadata(&watcher_req)
                && meta.len() > 0
                && let Ok(data) = std::fs::read(&watcher_req)
            {
                crate::kprintln!("-> [SANDBOX] Detected Crypto Portal request. Processing...");
                let _ = std::fs::write(&watcher_req, b"");

                let response = match KernelCrypto::process_portal_request(&data) {
                    Ok(res_bytes) => {
                        let mut out = vec![0u8]; // 0 = SUCCESS
                        out.extend_from_slice(&res_bytes);
                        out
                    }
                    Err(err_str) => {
                        let mut out = vec![1u8]; // 1 = ERROR
                        out.extend_from_slice(err_str.as_bytes());
                        out
                    }
                };

                let _ = std::fs::write(&watcher_tmp_res, response);
                crate::kprintln!("-> [SANDBOX] Crypto Portal response written to temporary file.");

                let _ = std::fs::remove_file(&watcher_res);
                let _ = std::fs::rename(&watcher_tmp_res, &watcher_res);
                crate::kprintln!(
                    "-> [SANDBOX] Crypto Portal response moved to final destination: {}",
                    watcher_res.display()
                );
            }
            thread::sleep(Duration::from_micros(50));
        }
    });

    Ok(handle)
}
