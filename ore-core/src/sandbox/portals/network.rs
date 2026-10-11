use super::super::net::execute_ore_fetch;
use crate::registry::NetworkRule;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(serde::Deserialize)]
struct JsFetch {
    method: String,
    url: String,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    body: String,
    filename: String,
}

pub fn spawn_network_portal(
    host_tmp_dir: &Path,
    stop_signal: Arc<AtomicBool>,
    rules: Vec<NetworkRule>,
    network_enabled: bool,
    localhost_access: bool,
) -> Result<JoinHandle<()>> {
    let network_dir = host_tmp_dir.join(".ore_network");
    std::fs::create_dir_all(&network_dir)?;

    let legacy_req = network_dir.join("req.json");
    let legacy_res = network_dir.join("res.bin");

    let _ = std::fs::write(&legacy_req, b"");
    let _ = std::fs::write(&legacy_res, b"");

    let net_watcher_dir = network_dir;
    let net_stop = stop_signal;
    let net_rules = rules;
    let net_enabled = network_enabled;
    let net_localhost = localhost_access;
    let net_closure_tmp = host_tmp_dir.to_path_buf();

    let handle = thread::spawn(move || {
        crate::kprintln!("-> [SANDBOX] Concurrent Network Portal Watcher started...");

        while !net_stop.load(Ordering::Relaxed) {
            if let Ok(entries) = std::fs::read_dir(&net_watcher_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }

                    let file_name = match path.file_name().and_then(|n| n.to_str()) {
                        Some(name) => name.to_string(),
                        None => continue,
                    };

                    let is_concurrent =
                        file_name.starts_with("req_") && file_name.ends_with(".json");
                    let is_legacy = file_name == "req.json";

                    if !is_concurrent && !is_legacy {
                        continue;
                    }

                    if let Ok(meta) = std::fs::metadata(&path)
                        && meta.len() > 0
                        && let Ok(data) = std::fs::read_to_string(&path)
                    {
                        let req = match serde_json::from_str::<JsFetch>(&data) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };

                        let (res_path, meta_path) = if is_concurrent {
                            let id_part = file_name
                                .strip_prefix("req_")
                                .and_then(|s| s.strip_suffix(".json"))
                                .unwrap_or("");

                            let _ = std::fs::remove_file(&path);
                            (
                                net_watcher_dir.join(format!("res_{}.bin", id_part)),
                                net_watcher_dir.join(format!("res_{}.meta", id_part)),
                            )
                        } else {
                            let _ = std::fs::write(&path, b"");
                            (
                                net_watcher_dir.join("res.bin"),
                                net_watcher_dir.join("res.meta"),
                            )
                        };

                        let rules_clone = net_rules.clone();
                        let closure_tmp = net_closure_tmp.clone();

                        thread::spawn(move || {
                            let parsed_url = match reqwest::Url::parse(&req.url) {
                                Ok(u) => u,
                                Err(e) => {
                                    crate::kprintln!(
                                        "-> [SANDBOX BLOCKED] Invalid URL format provided by Agent: {}",
                                        e
                                    );
                                    let _ = std::fs::write(&res_path, b"1|Invalid URL");
                                    return;
                                }
                            };

                            let host_only = parsed_url.host_str().unwrap_or("").to_string();

                            crate::kprintln!(
                                "-> [CONCURRENT NET INTERCEPT] Processing {} to {}",
                                req.method.to_uppercase(),
                                host_only
                            );

                            let meta_path_clone = meta_path.clone();
                            let res_path_clone = res_path.clone();

                            let fetch_result = execute_ore_fetch(
                                &req.method.to_uppercase(),
                                &parsed_url,
                                req.headers.as_ref(),
                                req.body.as_bytes(),
                                &req.filename,
                                net_enabled,
                                net_localhost,
                                &rules_clone,
                                &closure_tmp,
                                move |meta| {
                                    if let Ok(meta_json) = serde_json::to_string(meta) {
                                        let tmp_meta = meta_path_clone.with_extension("tmp");
                                        let _ = std::fs::write(&tmp_meta, meta_json.as_bytes());
                                        let _ = std::fs::rename(&tmp_meta, &meta_path_clone);
                                    }

                                    let tmp_res = res_path_clone.with_extension("tmp");
                                    let _ = std::fs::write(&tmp_res, b"0");
                                    let _ = std::fs::rename(&tmp_res, &res_path_clone);
                                },
                            );

                            if let Err((_code, err_msg)) = fetch_result {
                                let response_data = format!("1|{}", err_msg);
                                let tmp_res = res_path.with_extension("tmp");
                                let _ = std::fs::write(&tmp_res, response_data.as_bytes());
                                let _ = std::fs::rename(&tmp_res, &res_path);
                            }
                        });
                    }
                }
            }

            thread::sleep(Duration::from_micros(1000));
        }
    });

    Ok(handle)
}
