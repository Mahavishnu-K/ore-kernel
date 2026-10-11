use super::params::ExecuteParams;
use anyhow::Result;
use wasmtime::{Engine, Module};

/// Ahead-Of-Time (AOT) .cwasm compilation, validation, and zero-RAM mmap loader
pub fn load_or_compile_module(engine: &Engine, params: &ExecuteParams) -> Result<Module> {
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
            crate::kprintln!("-> [SANDBOX] .wasm is newer than .cwasm. Invalidating AOT Cache...");
        }
    }

    if use_cache {
        crate::kprintln!("-> [SANDBOX] AOT Cache Hit. Bypassing JIT Compiler...");
        // deserialize_file uses OS `mmap` under the hood. ZERO RAM BLOAT.
        unsafe { Ok(Module::deserialize_file(engine, &cwasm_path)?) }
    } else {
        crate::kprintln!("-> [SANDBOX] Cold Start. JIT Compiling WASM to Native Machine Code...");
        let compiled_module = Module::new(engine, &params.wasm_binary)?;

        crate::kprintln!("-> [SANDBOX] Saving AOT .cwasm cache to disk for future executions...");
        if let Ok(serialized_bytes) = compiled_module.serialize() {
            let _ = std::fs::write(&cwasm_path, serialized_bytes);
        }

        Ok(compiled_module)
    }
}
