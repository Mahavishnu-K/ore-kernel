use crate::crypto::KernelCrypto;
use crate::linker::{HasLinkerState, LinkerState};
use crate::registry::NetworkRule;

use anyhow::{Error, Result};
use wasmtime::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store, Table, TableType};
use wasmtime_wasi::p1::{WasiP1Ctx, add_to_linker_sync};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

// THE MASTER SANDBOX STATE
// Holds both the OS system calls (WASI) and the Dynamic Linker Registry
pub struct OreSandboxState {
    pub wasi: WasiP1Ctx,
    pub linker: LinkerState,
}

impl HasLinkerState for OreSandboxState {
    fn linker_state(&self) -> &LinkerState {
        &self.linker
    }

    fn linker_state_mut(&mut self) -> &mut LinkerState {
        &mut self.linker
    }
}

pub struct ExecuteParams {
    pub tool_name: String,
    pub wasm_binary: Vec<u8>,
    pub fuel_limit: u64,
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub inception: Option<(String, String)>,
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
    pub network_enabled: bool,
    pub allow_localhost_access: bool,
    pub network_rules: Vec<NetworkRule>,
    pub dynamic_vfs_path: Option<String>,
    pub wasm_path: std::path::PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OreResponseMeta {
    pub status: u16,
    pub status_text: String,
    pub headers: std::collections::HashMap<String, String>,
    pub cookies: std::collections::HashMap<String, String>,
}

struct TempDirGuard {
    path: std::path::PathBuf,
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
            crate::kprintln!("-> [SANDBOX VFS] Ephemeral directory destroyed.");
        }
    }
}

struct ThreadJoinGuard {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    crypto_handle: Option<std::thread::JoinHandle<()>>,
    net_handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ThreadJoinGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.crypto_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.net_handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct WasmSandbox {
    engine: Engine,
}

impl Default for WasmSandbox {
    fn default() -> Self {
        Self::new().expect("Failed to initialize WASM Sandbox Engine")
    }
}

impl WasmSandbox {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();

        config.consume_fuel(true);

        config.wasm_component_model(true);

        let engine = Engine::new(&config)?;
        Ok(Self { engine })
    }

    /// The "Inception" Execution (Happens per-request)
    pub fn execute(&self, params: ExecuteParams) -> Result<String> {
        let mut linker: Linker<OreSandboxState> = Linker::new(&self.engine);

        // Tells WASI how to find its context inside our master state
        add_to_linker_sync(&mut linker, |state| &mut state.wasi)?;

        // INJECT THE ORE DYNAMIC LINKER! (ore-ld)
        crate::linker::add_to_linker(&mut linker)?;

        let network_enabled = params.network_enabled;
        let localhost_access = params.allow_localhost_access;
        let rules = params.network_rules.clone();

        // CREATE A DEDICATED TEMP DIRECTORY FOR THIS EXECUTION
        let exec_id = uuid::Uuid::new_v4().to_string();
        let mut host_tmp_dir = crate::get_ore_dir().join("tmp").join(&exec_id);
        std::fs::create_dir_all(&host_tmp_dir)?;

        let sanitize_unc = |p: std::path::PathBuf| -> std::path::PathBuf {
            let s = p.to_string_lossy().to_string();
            if let Some(stripped) = s.strip_prefix(r#"\\?\"#) {
                std::path::PathBuf::from(stripped)
            } else {
                p
            }
        };

        let canon_tmp = std::fs::canonicalize(&host_tmp_dir).unwrap_or(host_tmp_dir);
        host_tmp_dir = sanitize_unc(canon_tmp);

        // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
        let crypto_dir = host_tmp_dir.join(".ore_crypto");
        std::fs::create_dir_all(&crypto_dir)?;

        // ORE INCEPTION CRYPTO PORTAL (HOST BACKEND)
        let portal_req = crypto_dir.join("req.bin");
        let portal_res = crypto_dir.join("res.bin");
        let portal_tmp_res = crypto_dir.join("res.tmp");

        // Pre-create the files to prevent O_CREAT ENOTSUP issues in QuickJS on Windows
        let _ = std::fs::write(&portal_req, b"");
        let _ = std::fs::write(&portal_res, b"");

        let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let watcher_req = portal_req.clone();
        let watcher_res = portal_res.clone();
        let watcher_tmp_res = portal_tmp_res.clone();
        let watcher_stop = stop_signal.clone();

        // Spawn a low-latency thread polling the VFS crypto queue while WASM executes
        // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
        let crypto_thread = std::thread::spawn(move || {
            crate::kprintln!(
                "-> [SANDBOX] Crypto Portal Thread started. Polling for requests in {}...",
                watcher_req.display()
            );
            while !watcher_stop.load(std::sync::atomic::Ordering::Relaxed) {
                // Read the request file
                if let Ok(meta) = std::fs::metadata(&watcher_req)
                    && meta.len() > 0
                    && let Ok(data) = std::fs::read(&watcher_req)
                {
                    crate::kprintln!("-> [SANDBOX] Detected Crypto Portal request. Processing...");
                    // Truncate immediately to acknowledge read to JS
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

                    // prevent partial host writes from being read by the guest (atomicity)

                    // writes the response to a temporary file first, then renames it to ensure atomicity
                    let _ = std::fs::write(&watcher_tmp_res, response);
                    crate::kprintln!(
                        "-> [SANDBOX] Crypto Portal response written to temporary file."
                    );

                    // Windows NTFS safety: windows atomic rename is not guaranteed to be atomic, so we remove the old file first
                    let _ = std::fs::remove_file(&watcher_res);
                    let _ = std::fs::rename(&watcher_tmp_res, &watcher_res);
                    crate::kprintln!(
                        "-> [SANDBOX] Crypto Portal response moved to final destination: {}",
                        watcher_res.display()
                    );
                }
                // Sleep 50 microseconds to minimize CPU usage while keeping latency negligible
                std::thread::sleep(std::time::Duration::from_micros(50));
            }
        });

        // ORE INCEPTION NETWORK PORTAL (For JS/Python VFS Routing)
        // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
        let network_dir = host_tmp_dir.join(".ore_network");
        std::fs::create_dir_all(&network_dir)?;

        // Pre-create legacy registers for backward compatibility
        let legacy_req = network_dir.join("req.json");
        let legacy_res = network_dir.join("res.bin");

        let _ = std::fs::write(&legacy_req, b"");
        let _ = std::fs::write(&legacy_res, b"");

        let net_watcher_dir = network_dir.clone();

        let net_stop = stop_signal.clone();

        let net_rules = rules.clone();
        let net_enabled = network_enabled;
        let net_localhost = localhost_access;
        let net_closure_tmp = host_tmp_dir.clone();

        // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
        let network_thread = std::thread::spawn(move || {
            crate::kprintln!("-> [SANDBOX] Concurrent Network Portal Watcher started...");

            #[derive(serde::Deserialize)]
            struct JsFetch {
                method: String,
                url: String,
                #[serde(default)]
                headers: Option<std::collections::HashMap<String, String>>,
                #[serde(default)]
                body: String,
                filename: String,
            }

            while !net_stop.load(std::sync::atomic::Ordering::Relaxed) {
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

                        // Match both concurrent 'req_<id>.json' and legacy 'req.json'
                        let is_concurrent =
                            file_name.starts_with("req_") && file_name.ends_with(".json");
                        let is_legacy = file_name == "req.json";

                        if !is_concurrent && !is_legacy {
                            continue;
                        }

                        // Ensure file is populated
                        if let Ok(meta) = std::fs::metadata(&path)
                            && meta.len() > 0
                            && let Ok(data) = std::fs::read_to_string(&path)
                        {
                            // Parse JSON safely
                            let req = match serde_json::from_str::<JsFetch>(&data) {
                                Ok(r) => r,
                                Err(_) => continue, // Still being written by guest; retry next loop
                            };

                            // Determine target response and metadata files
                            let (res_path, meta_path) = if is_concurrent {
                                let id_part = file_name
                                    .strip_prefix("req_")
                                    .and_then(|s| s.strip_suffix(".json"))
                                    .unwrap_or("");

                                // Delete request file immediately to acknowledge receipt
                                let _ = std::fs::remove_file(&path);
                                (
                                    net_watcher_dir.join(format!("res_{}.bin", id_part)),
                                    net_watcher_dir.join(format!("res_{}.meta", id_part)),
                                )
                            } else {
                                // Legacy single register
                                let _ = std::fs::write(&path, b""); // Clear register
                                (
                                    net_watcher_dir.join("res.bin"),
                                    net_watcher_dir.join("res.meta"),
                                )
                            };

                            // Clone parameters for background execution
                            let rules_clone = net_rules.clone();
                            let closure_tmp = net_closure_tmp.clone();

                            // Spawn host worker for true concurrent downloading & token streaming
                            std::thread::spawn(move || {
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

                                let fetch_result = WasmSandbox::execute_ore_fetch(
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
                                        // Save metadata descriptor (status, headers, cookies) to res_<id>.meta
                                        if let Ok(meta_json) = serde_json::to_string(meta) {
                                            let tmp_meta = meta_path_clone.with_extension("tmp");
                                            let _ = std::fs::write(&tmp_meta, meta_json.as_bytes());
                                            let _ = std::fs::rename(&tmp_meta, &meta_path_clone);
                                        }

                                        // Signal success (headers ready) to sandbox so guest unblocks immediately!
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

                // 1ms sleep preserves host CPU while keeping network latency minimal
                std::thread::sleep(std::time::Duration::from_micros(1000));
            }
        });

        let _cleanup_guard = TempDirGuard {
            path: host_tmp_dir.clone(),
        };

        if let Some((filename, content)) = &params.inception {
            let script_path = host_tmp_dir.join(filename);
            std::fs::write(&script_path, content)?;
            crate::kprintln!(
                "-> [INCEPTION] Dynamic AI script materialized to VFS: /ore_tmp/{}",
                filename
            );
        }

        // We clone the path so the closure can use it
        let closure_rules = rules.clone();
        let closure_tmp_dir = host_tmp_dir.clone();

        // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
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

                // Helper closure to safely read a byte array from WASM memory
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

                    // Out-of-bounds check (prevents the Guest from crashing the Host kernel)
                    if end > data.len() {
                        return None;
                    }

                    Some(data[start..end].to_vec())
                };

                // Helper closure to safely convert a byte array to a string from WASM memory
                let read_string = |mem: &Memory,
                                   caller: &mut Caller<'_, OreSandboxState>,
                                   ptr: u32,
                                   len: u32|
                 -> Option<String> {
                    let bytes = read_bytes(mem, caller, ptr, len)?;
                    String::from_utf8(bytes).ok()
                };

                // Extract the LIVE parameters from the Agent's code!
                let method = match read_string(&memory, &mut caller, method_ptr, method_len) {
                    Some(m) => m.to_uppercase(),
                    None => return -1, // Memory error
                };

                let url = match read_string(&memory, &mut caller, url_ptr, url_len) {
                    Some(u) => u,
                    None => return -1, // Memory error
                };

                let filename = match read_string(&memory, &mut caller, filename_ptr, filename_len) {
                    Some(f) => f,
                    None => return -1, // Memory error
                };

                let body = read_bytes(&memory, &mut caller, body_ptr, body_len).unwrap_or_default();

                let parsed_url = match reqwest::Url::parse(&url) {
                    Ok(u) => u,
                    Err(e) => {
                        crate::kprintln!(
                            "-> [SANDBOX BLOCKED] Invalid URL format provided by Agent: {}",
                            e
                        );
                        return -1; // 400 Bad Request
                    }
                };

                let host_only = parsed_url.host_str().unwrap_or("");

                crate::kprintln!(
                    "-> [SANDBOX INTERCEPT] Guest requested {} to {}",
                    method,
                    host_only
                );

                let fetch_res = WasmSandbox::execute_ore_fetch(
                    &method,
                    &parsed_url,
                    None,
                    &body,
                    &filename,
                    network_enabled,
                    localhost_access,
                    &closure_rules,
                    &closure_tmp_dir,
                    |_| {},
                );
                match fetch_res {
                    Ok(meta) => meta.status as i32,
                    Err((code, _)) => code,
                }
            },
        )?;

        // Create a pipe to catch all console output
        let stdout_buf = MemoryOutputPipe::new(10 * 1024 * 1024);
        let stderr_buf = MemoryOutputPipe::new(10 * 1024 * 1024);

        let mut wasi_builder = WasiCtxBuilder::new();

        // Configure WASI (The OS boundary for the Sandbox)
        wasi_builder
            .stdout(stdout_buf.clone())
            .stderr(stderr_buf.clone())
            .args(&params.args);

        if params
            .args
            .iter()
            .any(|arg| arg == "python" || arg.contains("system-py") || arg.ends_with(".py"))
        {
            wasi_builder.env("PYTHONUNBUFFERED", "1");

            // If it's a Bundled Tool (wasi-vfs)
            let mut pypath = String::from("/packages:/app");

            // If it's Inception Mode with JIT Requirements (Mounted VFS)
            if let Some(dyn_path) = params.dynamic_vfs_path {
                pypath.push(':');
                pypath.push_str(&dyn_path);
            }

            wasi_builder.env("PYTHONPATH", &pypath);

            crate::kprintln!(
                "-> [SANDBOX] Python runtime detected. Enabling unbuffered output and setting PYTHONPATH to '{}'.",
                pypath
            );
        }

        if let Some(input_bytes) = params.stdin {
            let stdin_buf = MemoryInputPipe::new(bytes::Bytes::from(input_bytes));
            wasi_builder.stdin(stdin_buf);
        }

        match wasi_builder.preopened_dir(
            &host_tmp_dir,
            "/ore_tmp",
            DirPerms::all(),
            FilePerms::all(),
        ) {
            Ok(_) => {
                crate::kprintln!("-> [SANDBOX] Mounted ephemeral network cache to '/ore_tmp'");
            }
            Err(e) => {
                crate::kprintln!(
                    "-> [SANDBOX WARN] Failed to inject ephemeral network cache: {}",
                    e
                );
            }
        }

        // HOST WRITE PATHS (Mounted beautifully inside /workspace)
        for path in &params.allowed_write_paths {
            // Ensure the directory exists on the host
            std::fs::create_dir_all(path)?;

            let folder_name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("write_dir");

            let guest_path = format!("/workspace/{}", folder_name);

            let canon_path = std::fs::canonicalize(path).unwrap_or(std::path::PathBuf::from(path));

            let safe_path = sanitize_unc(canon_path);

            match wasi_builder.preopened_dir(
                &safe_path,
                &guest_path,
                DirPerms::all(),
                FilePerms::all(),
            ) {
                Ok(_) => {
                    crate::kprintln!(
                        "-> [SANDBOX] Mounted Host Write Path '{}' to Guest '{}'",
                        path,
                        guest_path
                    );
                }
                Err(e) => {
                    crate::kprintln!(
                        "-> [SANDBOX WARN] Failed to inject Write Path '{}': {}",
                        path,
                        e
                    );
                }
            }
        }

        // GLOBAL STANDARD LIBRARY MOUNT (For JS/TS Node.js API Polyfills)
        // We mount ~/.ore/runtimes/js_modules to /modules in the sandbox (Read-Only)
        // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
        if params.args.iter().any(|arg| arg == "quickjs") {
            let js_modules_dir = crate::get_ore_dir().join("runtimes").join("modules");
            if js_modules_dir.exists() {
                let canon_modules =
                    std::fs::canonicalize(&js_modules_dir).unwrap_or(js_modules_dir);
                let safe_modules = sanitize_unc(canon_modules);
                match wasi_builder.preopened_dir(
                    &safe_modules,
                    "/modules",
                    DirPerms::READ,
                    FilePerms::READ,
                ) {
                    Ok(_) => crate::kprintln!(
                        "-> [SANDBOX] Mounted JS Standard Library (Node.js Polyfills) to '/modules'"
                    ),
                    Err(e) => {
                        crate::kprintln!("-> [SANDBOX WARN] Failed to mount JS Modules: {}", e)
                    }
                }
            }

            wasi_builder.env("QUICKJS_MODULE_PATH", "/modules");
            wasi_builder.env("NODE_PATH", "/modules");
        }

        // HOST READ PATHS (STRICTLY READ-ONLY inside /workspace) - NEVER DELETED
        for path in &params.allowed_read_paths {
            std::fs::create_dir_all(path).unwrap_or_default();

            let folder_name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("read_dir");

            let guest_path = format!("/workspace/{}", folder_name);

            let canon_path = std::fs::canonicalize(path).unwrap_or(std::path::PathBuf::from(path));
            let safe_path = sanitize_unc(canon_path);

            // Manually inject with stripped permissions!
            match wasi_builder.preopened_dir(
                &safe_path,
                &guest_path,
                DirPerms::READ,
                FilePerms::READ,
            ) {
                Ok(_) => {
                    crate::kprintln!(
                        "-> [SANDBOX] Mounted Host Read Path (STRICT READ-ONLY): '{}' to '{}'",
                        path,
                        guest_path
                    );
                }
                Err(e) => {
                    crate::kprintln!(
                        "-> [SANDBOX WARN] Failed to inject Read-Only Path '{}': {}",
                        path,
                        e
                    );
                }
            }
        }

        let wasi_ctx = wasi_builder.build_p1();

        let sandbox_state = OreSandboxState {
            wasi: wasi_ctx,
            linker: LinkerState::default(),
        };

        // Create the isolated State Store
        let mut store = Store::new(&self.engine, sandbox_state);

        let table_type: TableType = TableType::new(wasmtime::RefType::FUNCREF, 1024, Some(65536));
        let os_table = Table::new(&mut store, table_type, wasmtime::Ref::Func(None))
            .expect("FATAL: Failed to create OS Routing Table");

        store.data_mut().linker_state_mut().os_table = Some(os_table);

        linker
            .define(&mut store, "env", "__indirect_function_table", os_table)
            .expect("FATAL: Failed to inject OS Table");

        // Fuel Injection! Sandbox will panic if it exceeds this CPU instruction limit.
        store.set_fuel(params.fuel_limit)?;

        // THE AOT (AHEAD-OF-TIME) COMPILATION CACHE
        let cwasm_path = params.wasm_path.with_extension("cwasm");

        let mut use_cache = false;
        if cwasm_path.exists()
            && let (Ok(wasm_meta), Ok(cwasm_meta)) = (
                std::fs::metadata(&params.wasm_path),
                std::fs::metadata(&cwasm_path),
            )
        {
            if let (Ok(wasm_time), Ok(cwasm_time)) = (wasm_meta.modified(), cwasm_meta.modified())
                && cwasm_time >= wasm_time
            {
                use_cache = true;
            } else {
                crate::kprintln!(
                    "-> [SANDBOX] .wasm is newer than .cwasm. Invalidating AOT Cache..."
                );
            }
        }

        let module = if use_cache {
            crate::kprintln!("-> [SANDBOX] AOT Cache Hit. Bypassing JIT Compiler...");

            // deserialize_file uses OS `mmap` under the hood. ZERO RAM BLOAT.
            unsafe { Module::deserialize_file(&self.engine, &cwasm_path)? }
        } else {
            crate::kprintln!(
                "-> [SANDBOX] Cold Start. JIT Compiling WASM to Native Machine Code..."
            );
            let compiled_module = Module::new(&self.engine, &params.wasm_binary)?;

            crate::kprintln!(
                "-> [SANDBOX] Saving AOT .cwasm cache to disk for future executions..."
            );
            if let Ok(serialized_bytes) = compiled_module.serialize() {
                let _ = std::fs::write(&cwasm_path, serialized_bytes);
            }

            compiled_module
        };

        // THE WASMEDGE SOCKET FIX (DYNAMIC SHADOWING)
        // WasmEdge's QuickJS binary expects legacy socket signatures. Wasmtime 45.0
        // rejects this. We dynamically read what signature it wants and stub it out!
        linker.allow_shadowing(true); // Allow the Kernel to override WASI defaults

        for import in module.imports() {
            if import.module() == "wasi_snapshot_preview1"
                && import.name() == "sock_accept"
                && let Some(func_ty) = import.ty().func()
            {
                let dummy_func =
                    wasmtime::Func::new(&mut store, func_ty.clone(), |_, _, results| {
                        // WASI functions return i32 status codes. 52 = ENOSYS (Function Not Implemented)
                        if !results.is_empty() {
                            results[0] = wasmtime::Val::I32(52);
                        }
                        Ok(())
                    });
                linker.define(&mut store, import.module(), import.name(), dummy_func)?;
                crate::kprintln!(
                    "-> [SANDBOX] Dynamically shadowed legacy WasmEdge socket: 'sock_accept'"
                );
            }
        }

        linker.allow_shadowing(false);

        // THE SYSCALL STUBBER (Fixes WasmEdge and proprietary imports)
        // Automatically stubs out any unknown host functions with safe Traps so the VM can boot!
        linker.define_unknown_imports_as_traps(&module)?;

        let instance = linker.instantiate(&mut store, &module)?;
        let start_func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

        crate::kprintln!(
            "-> [SANDBOX] Booting Virtual Machine (Fuel Limit: {} instructions)...",
            params.fuel_limit
        );

        let thread_guard = ThreadJoinGuard {
            stop: stop_signal.clone(),
            crypto_handle: Some(crypto_thread),
            net_handle: Some(network_thread),
        };

        let exec_result = start_func.call(&mut store, ());

        // Stop the crypto thread and wait for it to finish
        drop(thread_guard);

        // Extraction & Destruction
        // Drop the store explicitly so the WritePipes finish cleanly
        drop(store);

        let stdout_bytes = stdout_buf.contents();

        let stderr_bytes = stderr_buf.contents();

        let mut final_output = String::from_utf8_lossy(&stdout_bytes).to_string();
        let error_output = String::from_utf8_lossy(&stderr_bytes).to_string();

        if !error_output.is_empty() {
            final_output.push_str("\n--- STDERR ---\n");
            final_output.push_str(&error_output);
        }

        match exec_result {
            Ok(_) => {
                crate::kprintln!("-> [SANDBOX] Execution completed safely.");
                Ok(final_output)
            }
            Err(e) => {
                // By using {:#}, anyhow prints the ENTIRE error chain, exposing the root cause!
                let err_msg = format!("{:#}", e);
                if err_msg.contains("out of fuel") || err_msg.contains("all fuel consumed") {
                    Err(Error::msg(
                        "Sandbox Trap: CPU Fuel Exhausted (Runaway AI or Infinite Loop Detected)",
                    ))
                } else if err_msg.contains("guest exit")
                    || err_msg.contains("runtime.exit")
                    || err_msg.contains("proc_exit")
                    || err_msg.contains("startWasi")
                {
                    // Normal WASI program exit code
                    crate::kprintln!("-> [SANDBOX] Program exited gracefully via OS syscall.");
                    Ok(final_output)
                } else {
                    crate::kprintln!("-> [SANDBOX TRAP] Execution halted: {}", e);
                    final_output.push_str(&format!("\nKERNEL ERROR: {}", e));
                    Ok(final_output)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_ore_fetch<F>(
        method: &str,
        parsed_url: &reqwest::Url,
        headers: Option<&std::collections::HashMap<String, String>>,
        body: &[u8],
        target_filename: &str,
        network_enabled: bool,
        localhost_access: bool,
        rules: &[NetworkRule],
        tmp_dir: &std::path::Path,
        on_headers: F,
    ) -> Result<OreResponseMeta, (i32, String)>
    where
        F: FnOnce(&OreResponseMeta),
    {
        // Global Network Checks
        if !network_enabled {
            crate::kprintln!("-> [FIREWALL] Network access globally disabled.");
            return Err((
                -1,
                "ORE Firewall: Network access globally disabled".to_string(),
            ));
        }

        // Security: Path Traversal Check
        let safe_filename = target_filename
            .strip_prefix(".ore_network/")
            .unwrap_or(target_filename);

        if safe_filename.contains('/')
            || safe_filename.contains('\\')
            || safe_filename.contains("..")
        {
            crate::kprintln!("-> [SANDBOX ERROR] Invalid filename. Path traversal blocked.");
            return Err((-1, "ORE Security: Path traversal blocked".to_string()));
        }

        let host_only = parsed_url.host_str().unwrap_or("");
        let is_local = host_only == "localhost"
            || host_only == "127.0.0.1"
            || host_only == "0.0.0.0"
            || host_only == "[::1]";

        if !localhost_access && is_local {
            crate::kprintln!("-> [FIREWALL] Localhost access is disabled.");
            return Err((
                -1,
                format!("ORE Firewall: Localhost access blocked for '{}'", host_only),
            ));
        }

        // Domain & Method Whitelist Check
        let mut is_allowed = false;
        for rule in rules {
            if rule.domain == host_only || rule.domain == "*" {
                if rule.allowed_methods.contains(&method.to_string())
                    || rule.allowed_methods.contains(&"*".to_string())
                {
                    is_allowed = true;
                    break;
                } else {
                    crate::kprintln!(
                        "-> [FIREWALL] Domain matched, but Method '{}' is FORBIDDEN. (Allowed: {:?})",
                        method,
                        rule.allowed_methods
                    );
                    return Err((
                        -2,
                        format!(
                            "ORE Firewall: Method '{}' not allowed for '{}'",
                            method, host_only
                        ),
                    ));
                }
            }
        }

        if !is_allowed {
            crate::kprintln!("-> [FIREWALL] Domain '{}' is not whitelisted.", host_only);
            return Err((
                -1,
                format!("ORE Firewall: Domain '{}' is not whitelisted", host_only),
            ));
        }

        crate::kprintln!(
            "-> [SANDBOX APPROVED] Routing {} to {} safely via ORE...",
            method,
            parsed_url
        );

        // Execute Request
        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                crate::kprintln!("-> [SANDBOX HTTP ERROR] Could not build HTTP client: {}", e);
                return Err((-3, format!("Could not build HTTP client: {}", e)));
            }
        };

        let http_method = match reqwest::Method::from_bytes(method.as_bytes()) {
            Ok(m) => m,
            Err(e) => {
                crate::kprintln!(
                    "-> [SANDBOX HTTP ERROR] Invalid HTTP method '{}': {}",
                    method,
                    e
                );
                return Err((-2, format!("Invalid HTTP method: {}", method)));
            }
        };

        let mut request = client.request(http_method, parsed_url.clone());

        // Forward request headers
        if let Some(hdrs) = headers {
            for (k, v) in hdrs {
                if let (Ok(h_name), Ok(h_val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(v),
                ) {
                    request = request.header(h_name, h_val);
                }
            }
        }

        if !body.is_empty() {
            request = request.body(body.to_vec());
        }

        let mut response = match request.send() {
            Ok(res) => res,
            Err(e) => {
                crate::kprintln!("-> [SANDBOX HTTP ERROR] {}", e);
                return Err((-3, format!("HTTP request failed: {}", e)));
            }
        };

        // Extract Real Status & Response Headers & Cookies
        let status_u16 = response.status().as_u16();
        let status_text = response
            .status()
            .canonical_reason()
            .unwrap_or("OK")
            .to_string();

        let mut headers_map = std::collections::HashMap::new();
        for (k, v) in response.headers().iter() {
            if let Ok(v_str) = v.to_str() {
                headers_map.insert(k.as_str().to_string(), v_str.to_string());
            }
        }

        let mut cookies_map = std::collections::HashMap::new();
        for val in response.headers().get_all(reqwest::header::SET_COOKIE) {
            if let Ok(val_str) = val.to_str()
                && let Some(cookie_pair) = val_str.split(';').next()
            {
                let mut parts = cookie_pair.splitn(2, '=');
                if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                    cookies_map.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }

        let meta = OreResponseMeta {
            status: status_u16,
            status_text: status_text.clone(),
            headers: headers_map,
            cookies: cookies_map,
        };

        // Notify caller that response metadata is ready (unblocks guest for real-time streaming)
        on_headers(&meta);

        // Streaming to VFS with flush
        let file_dest = tmp_dir.join(target_filename);
        let done_dest = tmp_dir.join(format!("{}.done", target_filename));

        if let Some(parent) = file_dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut file = match std::fs::File::create(&file_dest) {
            Ok(f) => f,
            Err(e) => {
                crate::kprintln!(
                    "-> [SANDBOX I/O ERROR] Error creating network request file: {}",
                    e
                );
                let _ = std::fs::write(&done_dest, format!("error:{}", e));
                return Err((-4, format!("Error creating file: {}", e)));
            }
        };

        use std::io::{Read, Write};
        let mut buf = [0u8; 8192];
        loop {
            match response.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = file.write_all(&buf[..n]) {
                        crate::kprintln!("-> [SANDBOX I/O ERROR] Failed to save chunk: {}", e);
                        let _ = std::fs::write(&done_dest, format!("error:{}", e));
                        return Err((-4, format!("Failed to write chunk: {}", e)));
                    }
                    let _ = file.flush();
                }
                Err(e) => {
                    crate::kprintln!("-> [SANDBOX I/O ERROR] Stream interrupted: {}", e);
                    let _ = std::fs::write(&done_dest, format!("error:{}", e));
                    return Err((-3, format!("Stream interrupted: {}", e)));
                }
            }
        }
        let _ = file.flush();

        // Write stream completion marker
        let _ = std::fs::write(&done_dest, b"ok");

        crate::kprintln!(
            "-> [SANDBOX HTTP] Success ({} {}). Saved response securely to VFS as '{}'.",
            status_u16,
            status_text,
            target_filename
        );

        Ok(meta)
    }
}
