use super::net::execute_ore_fetch;
use super::params::ExecuteParams;
use super::state::OreSandboxState;
use crate::linker::HasLinkerState;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use wasmtime::{AsContext, Caller, Extern, Linker, Memory, Module, Store, Table, TableType};

fn read_mem_bytes(mem: &Memory, caller: &impl AsContext, ptr: u32, len: u32) -> Option<Vec<u8>> {
    if len == 0 {
        return Some(vec![]);
    }
    let data = mem.data(caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    if end > data.len() {
        return None;
    }
    Some(data[start..end].to_vec())
}

fn read_mem_string(mem: &Memory, caller: &impl AsContext, ptr: u32, len: u32) -> Option<String> {
    let bytes = read_mem_bytes(mem, caller, ptr, len)?;
    String::from_utf8(bytes).ok()
}

fn parse_headers_json(headers_str: Option<&str>) -> Result<Option<HashMap<String, String>>, i32> {
    match headers_str {
        Some(s) if !s.trim().is_empty() => {
            match serde_json::from_str::<HashMap<String, String>>(s) {
                Ok(h) => Ok(Some(h)),
                Err(e) => {
                    crate::kprintln!("-> [SANDBOX WARN] Malformed headers JSON: {}", e);
                    Err(-1)
                }
            }
        }
        _ => Ok(None),
    }
}

/// Injects the `ore::fetch` host syscall into the Linker
pub fn register_ore_fetch(
    linker: &mut Linker<OreSandboxState>,
    host_tmp_dir: &Path,
    params: &ExecuteParams,
) -> Result<()> {
    let network_enabled = params.network_enabled;
    let localhost_access = params.allow_localhost_access;
    let rules = params.network_rules.clone();
    let tmp_dir = host_tmp_dir.to_path_buf();

    // sync fetch closure for move into linker
    let rules_sync = rules.clone();
    let tmp_dir_sync = tmp_dir.clone();

    // Register the synchronous fetch function
    linker.func_wrap(
        "ore",
        "fetch",
        move |mut caller: Caller<'_, OreSandboxState>,
              method_ptr: u32,
              method_len: u32,
              url_ptr: u32,
              url_len: u32,
              headers_ptr: u32,
              headers_len: u32,
              body_ptr: u32,
              body_len: u32,
              filename_ptr: u32,
              filename_len: u32|
              -> i32 {
            let memory = match caller.get_export("memory") {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    crate::kprintln!(
                        "-> [SANDBOX ERROR] Failed to find 'memory' export. Invalid WASM."
                    );
                    return -1;
                }
            };

            let method = match read_mem_string(&memory, &caller, method_ptr, method_len) {
                Some(m) => m.to_uppercase(),
                None => return -1,
            };

            let url = match read_mem_string(&memory, &caller, url_ptr, url_len) {
                Some(u) => u,
                None => return -1,
            };

            let filename = match read_mem_string(&memory, &caller, filename_ptr, filename_len) {
                Some(f) => f,
                None => return -1,
            };

            let body = read_mem_bytes(&memory, &caller, body_ptr, body_len).unwrap_or_default();

            let parsed_url = match reqwest::Url::parse(&url) {
                Ok(u) => u,
                Err(e) => {
                    crate::kprintln!(
                        "-> [SANDBOX BLOCKED] Invalid URL format provided by Agent: {}",
                        e
                    );
                    return -1;
                }
            };

            // Parse headers safely: if len == 0, it's None. If len > 0, validate JSON.
            let headers: Option<HashMap<String, String>> = if headers_len > 0 {
                let headers_str = match read_mem_string(&memory, &caller, headers_ptr, headers_len)
                {
                    Some(s) => s,
                    None => return -1,
                };
                match parse_headers_json(Some(&headers_str)) {
                    Ok(h) => h,
                    Err(code) => return code,
                }
            } else {
                None
            };

            let meta_dest = tmp_dir_sync.join(format!("{}.meta", filename));

            let host_only = parsed_url.host_str().unwrap_or("");
            crate::kprintln!(
                "-> [SANDBOX SYNC INTERCEPT] Guest requested {} to {}",
                method,
                host_only
            );

            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                crate::kprintln!("-> [SANDBOX ERROR] Path traversal blocked: {}", filename);
                return -1;
            }

            let fetch_res = execute_ore_fetch(
                &method,
                &parsed_url,
                headers.as_ref(),
                &body,
                &filename,
                network_enabled,
                localhost_access,
                &rules_sync,
                &tmp_dir_sync,
                |meta| {
                    if let Ok(meta_json) = serde_json::to_string(meta) {
                        let _ = std::fs::write(&meta_dest, meta_json);
                    }
                },
            );

            match fetch_res {
                Ok(meta) => meta.status as i32,
                Err((code, _)) => code,
            }
        },
    )?;

    // Register the asynchronous fetch function
    linker.func_wrap(
        "ore",
        "fetch_async",
        move |mut caller: Caller<'_, OreSandboxState>,
              method_ptr: u32,
              method_len: u32,
              url_ptr: u32,
              url_len: u32,
              headers_ptr: u32,
              headers_len: u32,
              body_ptr: u32,
              body_len: u32,
              filename_ptr: u32,
              filename_len: u32|
              -> i32 {
            let memory = match caller.get_export("memory") {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    crate::kprintln!(
                        "-> [SANDBOX ERROR] Failed to find 'memory' export. Invalid WASM."
                    );
                    return -1;
                }
            };

            let method = match read_mem_string(&memory, &caller, method_ptr, method_len) {
                Some(m) => m.to_uppercase(),
                None => return -1,
            };

            let url = match read_mem_string(&memory, &caller, url_ptr, url_len) {
                Some(u) => u,
                None => return -1,
            };

            let filename = match read_mem_string(&memory, &caller, filename_ptr, filename_len) {
                Some(f) => f,
                None => return -1,
            };

            let body = read_mem_bytes(&memory, &caller, body_ptr, body_len).unwrap_or_default();

            let parsed_url = match reqwest::Url::parse(&url) {
                Ok(u) => u,
                Err(e) => {
                    crate::kprintln!(
                        "-> [SANDBOX BLOCKED] Invalid URL format provided by Agent: {}",
                        e
                    );
                    return -1;
                }
            };

            // Parse headers safely: if len == 0, it's None. If len > 0, validate JSON.
            let headers: Option<HashMap<String, String>> = if headers_len > 0 {
                let headers_str = match read_mem_string(&memory, &caller, headers_ptr, headers_len)
                {
                    Some(s) => s,
                    None => return -1,
                };
                match parse_headers_json(Some(&headers_str)) {
                    Ok(h) => h,
                    Err(code) => return code,
                }
            } else {
                None
            };

            if !network_enabled {
                crate::kprintln!("-> [FIREWALL] Network access globally disabled.");
                return -1;
            }

            let host_only = parsed_url.host_str().unwrap_or("");
            crate::kprintln!(
                "-> [SANDBOX ASYNC INTERCEPT] Guest requested {} to {}",
                method,
                host_only
            );

            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                crate::kprintln!("-> [SANDBOX ERROR] Path traversal blocked: {}", filename);
                return -1;
            }

            if let Err((code, msg)) = crate::sandbox::net::fetch::check_egress(
                &method,
                &parsed_url,
                localhost_access,
                &rules,
            ) {
                crate::kprintln!("-> [FIREWALL] {}", msg);
                return code;
            }

            // spwan a thread to handle the async fetch
            let rules_clone = rules.clone();
            let tmp_dir_clone = tmp_dir.clone();
            let filename_clone = filename.clone();
            let method_clone = method.clone();
            let parsed_url_clone = parsed_url.clone();
            let body_clone = body.clone();
            let headers_clone = headers.clone();

            crate::kprintln!(
                "-> [SANDBOX ASYNC FETCH QUEUED] {} {}",
                method,
                parsed_url.host_str().unwrap_or("")
            );

            std::thread::spawn(move || {
                let meta_dest = tmp_dir_clone.join(format!("{}.meta", filename_clone));
                let done_dest = tmp_dir_clone.join(format!("{}.done", filename_clone));

                let fetch_res = execute_ore_fetch(
                    &method_clone,
                    &parsed_url_clone,
                    headers_clone.as_ref(),
                    &body_clone,
                    &filename_clone,
                    network_enabled,
                    localhost_access,
                    &rules_clone,
                    &tmp_dir_clone,
                    |meta| {
                        // Writes metadata immediately upon receiving headers (unblocks live stream)
                        if let Ok(meta_json) = serde_json::to_string(meta) {
                            let tmp_meta = meta_dest.with_extension("tmp");
                            let _ = std::fs::write(&tmp_meta, meta_json);
                            let _ = std::fs::rename(&tmp_meta, &meta_dest);
                        }
                    },
                );

                // If upstream request fails entirely before writing body, write sentinel error into .done
                if let Err((code, err_msg)) = fetch_res {
                    let _ =
                        std::fs::write(&done_dest, format!("error:{} (code {})", err_msg, code));
                    let err_meta = serde_json::json!({
                        "status": 502,
                        "status_text": err_msg,
                        "headers": {},
                        "cookies": {}
                    });
                    let _ = std::fs::write(&meta_dest, err_meta.to_string());
                }
            });

            0
        },
    )?;

    Ok(())
}

/// Sets up the indirect OS table for dynamic linking
pub fn create_os_routing_table(
    linker: &mut Linker<OreSandboxState>,
    store: &mut Store<OreSandboxState>,
) -> Result<()> {
    let table_type: TableType = TableType::new(wasmtime::RefType::FUNCREF, 1024, Some(65536));
    let os_table = Table::new(&mut *store, table_type, wasmtime::Ref::Func(None))
        .expect("FATAL: Failed to create OS Routing Table");

    store.data_mut().linker_state_mut().os_table = Some(os_table);

    linker
        .define(&mut *store, "env", "__indirect_function_table", os_table)
        .expect("FATAL: Failed to inject OS Table");

    Ok(())
}
