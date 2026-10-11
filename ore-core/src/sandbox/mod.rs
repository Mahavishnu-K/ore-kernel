pub mod cache;
pub mod net;
pub mod params;
pub mod portals;
pub mod shims;
pub mod state;
pub mod vfs;

pub use params::{ExecuteParams, OreResponseMeta};
pub use state::OreSandboxState;

use anyhow::{Error, Result};
use wasmtime::{Config, Engine, Linker, Store};
use wasmtime_wasi::p1::add_to_linker_sync;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

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
        // 1. Initialize Linker & Host State
        let mut linker: Linker<OreSandboxState> = Linker::new(&self.engine);

        // Tells WASI how to find its context inside our master state
        add_to_linker_sync(&mut linker, |state| &mut state.wasi)?;

        // INJECT THE ORE DYNAMIC LINKER! (ore-ld)
        crate::linker::add_to_linker(&mut linker)?;

        // 2. Setup VFS directories & TempDirGuard
        let (host_tmp_dir, _cleanup_guard) = vfs::setup_tmp_directory()?;

        // 3. Start Background IPC Portals (Crypto & Network)
        let _portal_guard = portals::start_all(&host_tmp_dir, &params)?;

        // 4. Register Syscalls
        shims::register_ore_fetch(&mut linker, &host_tmp_dir, &params)?;

        // 5. Build WASI Context (Pipes, Mounts, Envs)
        let (wasi_ctx, stdout_buf, stderr_buf) = vfs::build_wasi_context(&host_tmp_dir, &params)?;

        // 6. Create State Store & Apply Fuel Limits
        let sandbox_state = OreSandboxState::new(wasi_ctx);
        let mut store = Store::new(&self.engine, sandbox_state);

        // 7. Inject OS Routing Table
        shims::create_os_routing_table(&mut linker, &mut store)?;

        // Fuel Injection! Sandbox will panic if it exceeds this CPU instruction limit.
        store.set_fuel(params.fuel_limit)?;

        // 8. AOT Module Resolution
        let module = cache::load_or_compile_module(&self.engine, &params)?;

        // 9. Legacy shims & trap stubs
        shims::register_legacy_sock_accept(&mut linker, &mut store, &module)?;
        linker.define_unknown_imports_as_traps(&module)?;

        // 10. Instantiate & Run
        let instance = linker.instantiate(&mut store, &module)?;
        let start_func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

        crate::kprintln!(
            "-> [SANDBOX] Booting Virtual Machine (Fuel Limit: {} instructions)...",
            params.fuel_limit
        );

        let exec_result = start_func.call(&mut store, ());

        // 11. Teardown Portals & Store (flush pipes)
        drop(_portal_guard);
        drop(store);

        // 12. Format Output
        Self::format_output(exec_result, stdout_buf, stderr_buf)
    }

    fn format_output(
        exec_result: std::result::Result<(), wasmtime::Error>,
        stdout_buf: MemoryOutputPipe,
        stderr_buf: MemoryOutputPipe,
    ) -> Result<String> {
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
        rules: &[crate::registry::NetworkRule],
        tmp_dir: &std::path::Path,
        on_headers: F,
    ) -> Result<OreResponseMeta, (i32, String)>
    where
        F: FnOnce(&OreResponseMeta),
    {
        net::execute_ore_fetch(
            method,
            parsed_url,
            headers,
            body,
            target_filename,
            network_enabled,
            localhost_access,
            rules,
            tmp_dir,
            on_headers,
        )
    }
}
