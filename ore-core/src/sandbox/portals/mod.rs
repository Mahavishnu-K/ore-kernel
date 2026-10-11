pub mod crypto;
pub mod network;

use super::params::ExecuteParams;
use super::state::ThreadJoinGuard;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub fn start_all(host_tmp_dir: &Path, params: &ExecuteParams) -> Result<ThreadJoinGuard> {
    let stop_signal = Arc::new(AtomicBool::new(false));

    let crypto_handle = crypto::spawn_crypto_portal(host_tmp_dir, stop_signal.clone())?;
    let net_handle = network::spawn_network_portal(
        host_tmp_dir,
        stop_signal.clone(),
        params.network_rules.clone(),
        params.network_enabled,
        params.allow_localhost_access,
    )?;

    Ok(ThreadJoinGuard::new(
        stop_signal,
        Some(crypto_handle),
        Some(net_handle),
    ))
}
