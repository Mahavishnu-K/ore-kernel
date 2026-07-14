use crate::registry::NetworkRule;

use anyhow::{Error, Result};
use wasmtime::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store};
use wasmtime_wasi::p1::{WasiP1Ctx, add_to_linker_sync};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

pub struct ExecuteParams {
    pub wasm_binary: Vec<u8>,
    pub fuel_limit: u64,
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
    pub network_enabled: bool,
    pub allow_localhost_access: bool,
    pub network_rules: Vec<NetworkRule>,
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
        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
        add_to_linker_sync(&mut linker, |state| state)?;

        let network_enabled = params.network_enabled;
        let localhost_access = params.allow_localhost_access;
        let rules = params.network_rules.clone();

        // CREATE A DEDICATED TEMP DIRECTORY FOR THIS EXECUTION
        let exec_id = uuid::Uuid::new_v4().to_string();
        let host_tmp_dir = crate::get_ore_dir().join("tmp").join(&exec_id);
        std::fs::create_dir_all(&host_tmp_dir)?;

        let _cleanup_guard = TempDirGuard {
            path: host_tmp_dir.clone(),
        };

        // We clone the path so the closure can use it
        let closure_tmp_dir = host_tmp_dir.clone();

        linker.func_wrap(
            "ore",
            "fetch",
            move |mut caller: Caller<'_, WasiP1Ctx>,
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
                let read_bytes = |mem: &Memory, caller: &mut Caller<'_, WasiP1Ctx>, ptr: u32, len: u32| -> Option<Vec<u8>> {
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
                let read_string = |mem: &Memory, caller: &mut Caller<'_, WasiP1Ctx>, ptr: u32, len: u32| -> Option<String> {
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

            match wasi_builder.preopened_dir(path, &guest_path, DirPerms::all(), FilePerms::all()) {
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

        // HOST READ PATHS (STRICTLY READ-ONLY inside /workspace) - NEVER DELETED
        for path in &params.allowed_read_paths {
            let folder_name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("read_dir");

            let guest_path = format!("/workspace/{}", folder_name);

            // Manually inject with stripped permissions!
            match wasi_builder.preopened_dir(path, &guest_path, DirPerms::READ, FilePerms::READ) {
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

        // Create the isolated State Store
        let mut store = Store::new(&self.engine, wasi_ctx);

        // Fuel Injection! Sandbox will panic if it exceeds this CPU instruction limit.
        store.set_fuel(params.fuel_limit)?;

        // JIT Compilation (Near-Instantaneous)
        let module = Module::new(&self.engine, &params.wasm_binary)?;

        let instance = linker.instantiate(&mut store, &module)?;
        let start_func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

        crate::kprintln!(
            "-> [SANDBOX] Booting Virtual Machine (Fuel Limit: {} instructions)...",
            params.fuel_limit
        );

        match start_func.call(&mut store, ()) {
            Ok(_) => crate::kprintln!("-> [SANDBOX] Execution completed safely."),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("out of fuel") {
                    return Err(Error::msg(
                        "Sandbox Trap: CPU Fuel Exhausted (Runaway AI or Infinite Loop Detected)",
                    ));
                } else if err_msg.contains("guest exit") {
                    // Normal WASI program exit code
                    crate::kprintln!("-> [SANDBOX] Program exited.");
                } else {
                    crate::kprintln!("-> [SANDBOX TRAP] Execution halted: {}", e);
                }
            }
        }

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

        Ok(final_output)
    }
}
