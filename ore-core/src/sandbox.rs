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
    pub cache_key: String,
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
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ThreadJoinGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
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

        // CRITICAL WINDOWS FIX: Canonicalize path for Windows WASI compatibility
        let canon_tmp = std::fs::canonicalize(&host_tmp_dir).unwrap_or(host_tmp_dir);
        host_tmp_dir = sanitize_unc(canon_tmp);

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
        let closure_tmp_dir = host_tmp_dir.clone();

        linker.func_wrap(
            "ore",
            "fetch",
            move |mut caller: Caller<'_, OreSandboxState>,
                  method_ptr: u32, method_len: u32,
                  url_ptr: u32, url_len: u32,
                  body_ptr: u32, body_len: u32,
                  filename_ptr: u32, filename_len: u32| -> i32 {

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(mem)) => mem,
                    _ => {
                        crate::kprintln!("-> [SANDBOX ERROR] Failed to find 'memory' export. Invalid WASM.");
                        return -1;
                    }
                };

                // Helper closure to safely read a byte array from WASM memory
                let read_bytes = |mem: &Memory, caller: &mut Caller<'_, OreSandboxState>, ptr: u32, len: u32| -> Option<Vec<u8>> {
                    if len == 0 { return Some(vec![]); }
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
                let read_string = |mem: &Memory, caller: &mut Caller<'_, OreSandboxState>, ptr: u32, len: u32| -> Option<String> {
                    let bytes = read_bytes(mem, caller, ptr, len)?;
                    String::from_utf8(bytes).ok()
                };

                // Extract the LIVE parameters from the Agent's code!
                let requested_method = match read_string(&memory, &mut caller, method_ptr, method_len) {
                    Some(m) => m.to_uppercase(),
                    None => return -1, // Memory error
                };

                let raw_requested_url = match read_string(&memory, &mut caller, url_ptr, url_len) {
                    Some(u) => u,
                    None => return -1, // Memory error
                };

                let target_filename = match read_string(&memory, &mut caller, filename_ptr, filename_len) {
                    Some(f) => f,
                    None => return -1, // Memory error
                };

                // Path Traversal Security: Prevent the guest from writing outside the tmp dir!
                if target_filename.contains('/') || target_filename.contains('\\') || target_filename.contains("..") {
                    crate::kprintln!("-> [SANDBOX ERROR] Invalid filename. Path traversal blocked.");
                    return -1;
                }

                let parsed_url = match reqwest::Url::parse(&raw_requested_url) {
                    Ok(u) => u,
                    Err(e) => {
                        crate::kprintln!("-> [SANDBOX BLOCKED] Invalid URL format provided by Agent: {}", e);
                        return -1; // 400 Bad Request
                    }
                };

                let host_only = parsed_url.host_str().unwrap_or("");

                crate::kprintln!(
                    "-> [SANDBOX INTERCEPT] Guest requested {} to {}", 
                    requested_method, host_only
                );

                if !network_enabled {
                    crate::kprintln!("-> [SANDBOX BLOCKED] Network access globally disabled.");
                    return -1; // 403 Forbidden
                }

                // Catch all common loopback/local addresses
                let is_local = host_only == "localhost" 
                    || host_only == "127.0.0.1" 
                    || host_only == "0.0.0.0" 
                    || host_only == "[::1]";

                if !localhost_access && is_local {
                    crate::kprintln!("-> [SANDBOX BLOCKED] Localhost access is disabled.");
                    return -1; // 403 Forbidden
                }

                // Scan the Manifest Rules
                let mut is_allowed = false;
                for rule in &rules {
                    if rule.domain == host_only || rule.domain == "*" {
                        if rule.allowed_methods.contains(&requested_method.to_string()) || rule.allowed_methods.contains(&"*".to_string()) {
                            is_allowed = true;
                            break;
                        } else {
                            crate::kprintln!(
                                "-> [SANDBOX BLOCKED] Domain matched, but Method '{}' is FORBIDDEN. (Allowed: {:?})", 
                                requested_method, rule.allowed_methods
                            );
                            return -2; // 405 Method Not Allowed
                        }
                    }
                }

                if !is_allowed {
                    crate::kprintln!("-> [SANDBOX BLOCKED] Domain '{}' is not whitelisted.", host_only);
                    return -1;
                }

                crate::kprintln!("-> [SANDBOX APPROVED] Routing {} request to {} safely via ORE...", requested_method, raw_requested_url);

                let client = match reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                {
                    Ok(c) => c,
                    Err(e) => {
                        crate::kprintln!("-> [SANDBOX HTTP ERROR] Could not build HTTP client: {}", e);
                        return -3; // Network error
                    }
                };

                let req_body = read_bytes(&memory, &mut caller, body_ptr, body_len).unwrap_or_default();

                let request = match requested_method.as_str() {
                    "GET" => client.get(&raw_requested_url),
                    "POST" => client.post(&raw_requested_url).body(req_body),
                    "PUT" => client.put(&raw_requested_url).body(req_body),
                    "DELETE" => client.delete(&raw_requested_url),
                    _ => return -2, // Method not allowed
                };

                let mut response = match request.send() {
                    Ok(res) => res,
                    Err(e) => {
                        crate::kprintln!("-> [SANDBOX HTTP ERROR] {}", e);
                        return -3; // Network error
                    }
                };

                // ZERO-RAM STREAMING DIRECTLY TO THE SSD!
                let file_dest = closure_tmp_dir.join(&target_filename);
                let mut file =  match std::fs::File::create(&file_dest) {
                    Ok(f) => f,
                    Err(e) => {
                        crate::kprintln!(
                            "-> [SANDBOX I/O ERROR] Error creating network request file: {}",
                            e
                        );
                        return -4;
                    }
                };

                // std::io::copy pulls bytes from the network and writes them straight to the disk.
                // It NEVER loads the whole file into RAM!
                if let Err(e) = std::io::copy(&mut response, &mut file) {
                    crate::kprintln!("-> [SANDBOX I/O ERROR] Failed to save file: {}", e);
                    return -4;
                }

                crate::kprintln!("-> [SANDBOX HTTP] Success. Saved response securely to VFS as '{}'.", target_filename);

                0 // 200 OK!
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
        if params.args.iter().any(|arg| arg == "quickjs") {
            let js_modules_dir = crate::get_ore_dir().join("runtimes").join("js_modules");
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

        let module = if cwasm_path.exists() {
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
            handle: Some(crypto_thread),
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
}
