use super::net::execute_ore_fetch;
use super::params::ExecuteParams;
use super::state::OreSandboxState;
use crate::linker::HasLinkerState;
use anyhow::Result;
use std::path::Path;
use wasmtime::{Caller, Extern, Linker, Memory, Module, Store, Table, TableType};

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

    linker.func_wrap(
        "ore",
        "fetch",
        move |mut caller: Caller<'_, OreSandboxState>,
              method_ptr: u32,
              method_len: u32,
              url_ptr: u32,
              url_len: u32,
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

            let read_bytes = |mem: &Memory,
                              caller: &mut Caller<'_, OreSandboxState>,
                              ptr: u32,
                              len: u32|
             -> Option<Vec<u8>> {
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
            };

            let read_string = |mem: &Memory,
                               caller: &mut Caller<'_, OreSandboxState>,
                               ptr: u32,
                               len: u32|
             -> Option<String> {
                let bytes = read_bytes(mem, caller, ptr, len)?;
                String::from_utf8(bytes).ok()
            };

            let method = match read_string(&memory, &mut caller, method_ptr, method_len) {
                Some(m) => m.to_uppercase(),
                None => return -1,
            };

            let url = match read_string(&memory, &mut caller, url_ptr, url_len) {
                Some(u) => u,
                None => return -1,
            };

            let filename = match read_string(&memory, &mut caller, filename_ptr, filename_len) {
                Some(f) => f,
                None => return -1,
            };

            let body = read_bytes(&memory, &mut caller, body_ptr, body_len).unwrap_or_default();

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

            let host_only = parsed_url.host_str().unwrap_or("");
            crate::kprintln!(
                "-> [SANDBOX INTERCEPT] Guest requested {} to {}",
                method,
                host_only
            );

            let fetch_res = execute_ore_fetch(
                &method,
                &parsed_url,
                None,
                &body,
                &filename,
                network_enabled,
                localhost_access,
                &rules,
                &tmp_dir,
                |_| {},
            );
            match fetch_res {
                Ok(meta) => meta.status as i32,
                Err((code, _)) => code,
            }
        },
    )?;

    Ok(())
}

/// Injects dynamic shadowing for legacy WasmEdge sock_accept
pub fn register_legacy_sock_accept(
    linker: &mut Linker<OreSandboxState>,
    store: &mut Store<OreSandboxState>,
    module: &Module,
) -> Result<()> {
    linker.allow_shadowing(true);

    for import in module.imports() {
        if import.module() == "wasi_snapshot_preview1"
            && import.name() == "sock_accept"
            && let Some(func_ty) = import.ty().func()
        {
            let dummy_func = wasmtime::Func::new(&mut *store, func_ty.clone(), |_, _, results| {
                if !results.is_empty() {
                    results[0] = wasmtime::Val::I32(52); // ENOSYS
                }
                Ok(())
            });
            linker.define(&mut *store, import.module(), import.name(), dummy_func)?;
            crate::kprintln!(
                "-> [SANDBOX] Dynamically shadowed legacy WasmEdge socket: 'sock_accept'"
            );
        }
    }

    linker.allow_shadowing(false);
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
