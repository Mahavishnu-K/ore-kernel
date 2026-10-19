pub mod cache;
pub mod net;
pub mod params;
pub mod portals;
pub mod shims;
pub mod state;
pub mod vfs;

pub use params::{ExecuteParams, OreResponseMeta};
pub use state::OreSandboxState;

use anyhow::Result;
use wasmtime::{Config, Engine, Linker, Store, Trap};
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
        // Initialize Linker & Host State
        let mut linker: Linker<OreSandboxState> = Linker::new(&self.engine);

        // Tells WASI how to find its context inside our master state
        add_to_linker_sync(&mut linker, |state| &mut state.wasi)?;

        // INJECT THE ORE DYNAMIC LINKER! (ore-ld)
        crate::linker::add_to_linker(&mut linker)?;

        // Setup VFS directories & TempDirGuard
        let (host_tmp_dir, _cleanup_guard) = vfs::setup_tmp_directory()?;

        // Start Background IPC Portals (Crypto & Network)
        let _portal_guard = portals::start_all(&host_tmp_dir, &params)?;

        // Register Syscalls
        shims::register_ore_fetch(&mut linker, &host_tmp_dir, &params)?;

        // Build WASI Context (Pipes, Mounts, Envs)
        let (wasi_ctx, stdout_buf, stderr_buf) = vfs::build_wasi_context(&host_tmp_dir, &params)?;

        // Create State Store & Apply Fuel Limits
        let sandbox_state = OreSandboxState::new(wasi_ctx);
        let mut store = Store::new(&self.engine, sandbox_state);

        // Inject OS Routing Table
        shims::create_os_routing_table(&mut linker, &mut store)?;

        // Fuel Injection! Sandbox will panic if it exceeds this CPU instruction limit.
        store.set_fuel(params.fuel_limit)?;

        // AOT Module Resolution
        let module = cache::load_or_compile_module(&self.engine, &params)?;

        // Legacy shims & trap stubs
        shims::register_legacy_sock_accept(&mut linker, &mut store, &module)?;
        linker.define_unknown_imports_as_traps(&module)?;

        // Instantiate & Run
        let instance = linker.instantiate(&mut store, &module)?;
        let start_func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

        crate::kprintln!(
            "-> [SANDBOX] Booting Virtual Machine (Fuel Limit: {} instructions)...",
            params.fuel_limit
        );

        let exec_result = start_func.call(&mut store, ());

        // Teardown Portals & Store (flush pipes)
        drop(_portal_guard);
        drop(store);

        // Format Output
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
            // Classify by error *type*, never by message text: the message includes the guest's
            // wasm backtrace, so a substring match (e.g. Zig's `start.startWasi`, Go's
            // `runtime.exit`) can mistake a trap for a clean exit.
            //
            // Every outcome returns the guest's stdout/stderr. Failures append exactly one
            // `KERNEL ERROR:` line, so agents can detect them the same way in every language.
            Err(e) => {
                // The guest called proc_exit (returning from main in C/C++/Zig/Go/Python, or
                // an explicit exit()). Status 0 is success; anything else is a failure,
                // e.g. Go panics exit with 2 and uncaught Python exceptions exit with 1.
                if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                    if exit.0 == 0 {
                        crate::kprintln!(
                            "-> [SANDBOX] Program exited gracefully via OS syscall (status 0)."
                        );
                    } else {
                        crate::kprintln!(
                            "-> [SANDBOX] Program exited with failure status {}.",
                            exit.0
                        );
                        final_output.push_str(&format!(
                            "\nKERNEL ERROR: Guest exited with status {}",
                            exit.0
                        ));
                    }
                    return Ok(final_output);
                }

                // The fuel limit (manifest `max_cpu_instructions`) ran out.
                if matches!(e.downcast_ref::<Trap>(), Some(Trap::OutOfFuel)) {
                    crate::kprintln!("-> [SANDBOX TRAP] CPU fuel exhausted.");
                    final_output.push_str(
                        "\nKERNEL ERROR: Sandbox Trap: CPU Fuel Exhausted (Runaway AI or Infinite Loop Detected)",
                    );
                    return Ok(final_output);
                }

                // Any other trap: Rust panic, C abort, out-of-bounds access, null or
                // mismatched indirect call (e.g. a stale ore-ld pointer), stack overflow...
                crate::kprintln!("-> [SANDBOX TRAP] Execution halted: {}", e);
                final_output.push_str(&format!("\nKERNEL ERROR: {}", e));
                Ok(final_output)
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

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime_wasi::I32Exit;

    fn run(exec_result: std::result::Result<(), wasmtime::Error>) -> String {
        WasmSandbox::format_output(
            exec_result,
            MemoryOutputPipe::new(1024),
            MemoryOutputPipe::new(1024),
        )
        .expect("format_output never returns Err")
    }

    #[test]
    fn normal_return_is_success() {
        assert!(!run(Ok(())).contains("KERNEL ERROR"));
    }

    #[test]
    fn exit_status_zero_is_success() {
        assert!(!run(Err(wasmtime::Error::new(I32Exit(0)))).contains("KERNEL ERROR"));
    }

    #[test]
    fn nonzero_exit_status_is_failure() {
        // e.g. Go panic (2) or uncaught Python exception (1)
        let out = run(Err(wasmtime::Error::new(I32Exit(2))));
        assert!(out.contains("KERNEL ERROR: Guest exited with status 2"));
    }

    #[test]
    fn out_of_fuel_is_reported() {
        let out = run(Err(wasmtime::Error::new(Trap::OutOfFuel)));
        assert!(out.contains("KERNEL ERROR: Sandbox Trap: CPU Fuel Exhausted"));
    }

    #[test]
    fn other_traps_are_failures() {
        let out = run(Err(wasmtime::Error::new(Trap::TableOutOfBounds)));
        assert!(out.contains("KERNEL ERROR"));
    }

    #[test]
    fn trap_backtrace_text_is_not_mistaken_for_exit() {
        // Regression: Zig traps carry `start.startWasi` in their backtrace, and Go traps can
        // carry `runtime.exit`. Only a typed I32Exit may count as an exit.
        for frame in ["start.startWasi", "runtime.exit", "proc_exit", "guest exit"] {
            let err = wasmtime::Error::new(Trap::IndirectCallToNull)
                .context(format!("wasm backtrace: 0: {}", frame));
            assert!(run(Err(err)).contains("KERNEL ERROR"), "frame `{}`", frame);
        }
    }

    #[test]
    fn exit_is_detected_through_added_context() {
        let err = wasmtime::Error::new(I32Exit(0)).context("wasm backtrace: ...");
        assert!(!run(Err(err)).contains("KERNEL ERROR"));
    }
}
