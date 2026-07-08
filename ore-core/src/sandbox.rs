use anyhow::{Error, Result};
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use wasi_common::WasiCtx;
use wasi_common::pipe::WritePipe;
use wasi_common::sync::{WasiCtxBuilder, add_to_linker};
use wasmtime::*;

pub struct ExecuteParams {
    pub wasm_binary: Vec<u8>,
    pub fuel_limit: u64,
    pub args: Vec<String>,
    pub allowed_read_paths: Vec<String>,
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

        // Deterministic CPU Profiling (The "Fuel" Gauge)
        // This physically prevents infinite loops and host lockups.
        config.consume_fuel(true);

        // Zero Network (WASI defaults to isolated)
        config.wasm_component_model(false);

        let engine = Engine::new(&config)?;
        Ok(Self { engine })
    }

    /// The "Inception" Execution (Happens per-request)
    pub fn execute(&self, params: ExecuteParams) -> Result<String> {
        let mut linker: Linker<WasiCtx> = Linker::new(&self.engine);
        add_to_linker(&mut linker, |s| s)?;

        // Create a pipe to catch all console output
        let stdout_buf = WritePipe::new_in_memory();
        let stderr_buf = WritePipe::new_in_memory();

        let mut wasi_builder = WasiCtxBuilder::new();

        // Configure WASI (The OS boundary for the Sandbox)
        wasi_builder
            .stdout(Box::new(stdout_buf.clone()))
            .stderr(Box::new(stderr_buf.clone()))
            .args(&params.args)?;

        // Capability-Based File System (cap-std mounts)
        for path in &params.allowed_read_paths {
            if let Ok(dir) = Dir::open_ambient_dir(path, ambient_authority()) {
                // Mount it as read-only inside the Sandbox at "/workspace"
                let guest_path = "/workspace";
                wasi_builder.preopened_dir(dir, guest_path)?;
                crate::kprintln!(
                    "-> [SANDBOX] Mounted host path '{}' to '{}'",
                    path,
                    guest_path
                );
            } else {
                crate::kprintln!("-> [SANDBOX WARN] Failed to mount host path '{}'", path);
            }
        }

        let wasi_ctx = wasi_builder.build();

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

        let stdout_bytes = stdout_buf
            .try_into_inner()
            .expect("Failed to extract stdout")
            .into_inner();

        let stderr_bytes = stderr_buf
            .try_into_inner()
            .expect("Failed to extract stderr")
            .into_inner();

        let mut final_output = String::from_utf8_lossy(&stdout_bytes).to_string();
        let error_output = String::from_utf8_lossy(&stderr_bytes).to_string();

        if !error_output.is_empty() {
            final_output.push_str("\n--- STDERR ---\n");
            final_output.push_str(&error_output);
        }

        Ok(final_output)
    }
}
