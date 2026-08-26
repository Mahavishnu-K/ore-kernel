pub mod linker_state;
pub mod mmu;
pub mod syscalls;

// Re-export the core types so `sandbox.rs` can use them easily
pub use linker_state::LinkerState;
pub use syscalls::HasLinkerState;

use anyhow::Result;
use wasmtime::Linker;

/// Injects the ORE Dynamic Linker (ore-ld) Host Functions into the Wasmtime environment.
/// This instantly gives the Agent the ability to perform C-ABI True Memory Fusion.
pub fn add_to_linker<T>(linker: &mut Linker<T>) -> Result<()>
where
    T: HasLinkerState + Send + 'static,
{
    crate::kprintln!("-> [ORE-LD] Initializing True Memory Fusion Subsystem...");

    // The C-ABI Native Linker (For Rust, C, C++, Zig)
    linker.func_wrap("env", "ore_dlopen", syscalls::trap_ore_dlopen)?;
    linker.func_wrap("env", "ore_dlsym", syscalls::trap_ore_dlsym)?;

    // The @ore/sdk Bridge (For Python & TS/JS)
    // We will build these syscalls next for Room-to-Room IPC!
    // linker.func_wrap("env", "ore_bridge_spawn", syscalls::trap_ore_bridge_spawn)?;
    // linker.func_wrap("env", "ore_bridge_call", syscalls::trap_ore_bridge_call)?;

    Ok(())
}
