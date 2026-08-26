use wasmtime::{Caller, Global, GlobalType, Memory, Mutability, Val, ValType};

/// Dynamically allocates RAM for the Plugin's .data segments without overwriting the Host's malloc
pub fn allocate_plugin_memory<T>(
    caller: &mut Caller<'_, T>,
    memory: &Memory,
    pages_needed: u64,
) -> Result<u32, String> {
    // We reborrow `caller` using `&mut *caller`
    match memory.grow(&mut *caller, pages_needed) {
        Ok(old_page_count) => {
            // WebAssembly pages are exactly 64KB (65536 bytes)
            // The plugin's memory base is the start of the newly allocated pages
            Ok((old_page_count * 65536) as u32)
        }
        Err(e) => Err(format!("Failed to grow Agent RAM for Plugin: {}", e)),
    }
}

/// Forges the WebAssembly Globals required by the LLVM C-ABI (-fPIC)
pub fn forge_pic_globals<T>(
    caller: &mut Caller<'_, T>,
    mem_base: u32,
    table_base: u32,
) -> Result<(Global, Global), String> {
    let mem_global = match Global::new(
        &mut *caller,
        GlobalType::new(ValType::I32, Mutability::Const), // Const, because base addresses don't change
        Val::I32(mem_base as i32),
    ) {
        Ok(g) => g,
        Err(e) => return Err(format!("Failed to create __memory_base: {}", e)),
    };

    let table_global = match Global::new(
        &mut *caller,
        GlobalType::new(ValType::I32, Mutability::Const),
        Val::I32(table_base as i32),
    ) {
        Ok(g) => g,
        Err(e) => return Err(format!("Failed to create __table_base: {}", e)),
    };

    Ok((mem_global, table_global))
}
