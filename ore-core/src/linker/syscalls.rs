use crate::linker::linker_state::{LinkerState, PluginMetadata};
use crate::linker::mmu::{allocate_plugin_memory, forge_pic_globals};
use ore_common::paths;
use wasmtime::{Caller, Linker, Module, Ref};

// [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
/// Trait to ensure the Caller's generic State contains our Linker Registry.
pub trait HasLinkerState {
    fn linker_state(&self) -> &LinkerState;
    fn linker_state_mut(&mut self) -> &mut LinkerState;
}

/// Stores the last linker error so the guest can read it back via `ore_dlerror`.
fn set_err<T: HasLinkerState>(caller: &mut Caller<'_, T>, msg: String) {
    caller.data_mut().linker_state_mut().last_error = Some(msg);
}

/// Returns a memory slab to the free list (used when dlopen fails after allocating it).
fn release_slab<T: HasLinkerState>(caller: &mut Caller<'_, T>, mem_base: u32, mem_pages: u64) {
    caller
        .data_mut()
        .linker_state_mut()
        .free_mem_slabs
        .push((mem_base, mem_pages));
}

// [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
/// SYSCALL: ore_dlopen
/// Dynamically loads a .wasi.so file, fuses its memory, and initializes it.
/// Returns: Handle ID (> 0) on success, or <= 0 on failure.
pub fn trap_ore_dlopen<T: HasLinkerState>(
    mut caller: Caller<'_, T>,
    filename_ptr: u32,
    filename_len: u32,
) -> i32 {
    // Safely extract the Host Agent's RAM
    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => {
            crate::kprintln!("-> [ORE-LD] FATAL: Host Agent did not export 'memory'.");
            set_err(
                &mut caller,
                "Host Agent did not export 'memory'.".to_string(),
            );
            return -1;
        }
    };

    // SCOPE BLOCK: Confine the immutable borrow of `caller` so it drops instantly!
    let filename = {
        let data = memory.data(&caller);
        let start = filename_ptr as usize;
        let end = start.checked_add(filename_len as usize).unwrap_or(0);

        if end > data.len() {
            crate::kprintln!("-> [ORE-LD] FATAL: Memory bounds violation reading dlopen filename.");
            set_err(
                &mut caller,
                "Memory bounds violation reading dlopen filename.".to_string(),
            );
            return -1;
        }

        match std::str::from_utf8(&data[start..end]) {
            Ok(s) => s.to_string(), // Copy into an Owned String!
            Err(_) => {
                crate::kprintln!("-> [ORE-LD] FATAL: Invalid UTF-8 in dlopen filename.");
                set_err(&mut caller, "Invalid UTF-8 in dlopen filename.".to_string());
                return -1;
            }
        }
    }; // The `data` reference is dropped here. `caller` is now completely unlocked!

    // File Security & I/O
    let plugin_path = paths::get_plugins_dir().join(&filename);
    if !plugin_path.exists() {
        crate::kprintln!(
            "-> [ORE-LD] ERROR: Plugin '{}' not found in ~/.ore/plugins/",
            filename
        );
        set_err(
            &mut caller,
            format!("Plugin '{}' not found in ~/.ore/plugins/", filename),
        );
        return -1;
    }

    let plugin_bytes = match std::fs::read(&plugin_path) {
        Ok(b) => b,
        Err(e) => {
            crate::kprintln!(
                "-> [ORE-LD] ERROR: Failed to read plugin {}: {}",
                filename,
                e
            );
            set_err(
                &mut caller,
                format!("Failed to read plugin {}: {}", filename, e),
            );
            return -1;
        }
    };

    let plugin_module = match Module::new(caller.engine(), &plugin_bytes) {
        Ok(m) => m,
        Err(e) => {
            crate::kprintln!(
                "-> [ORE-LD] ERROR: Failed to compile plugin {}: {}",
                filename,
                e
            );
            set_err(
                &mut caller,
                format!("Failed to compile plugin {}: {}", filename, e),
            );
            return -1;
        }
    };

    // Extract the Agent's Function Table (The Routing Brain)
    let agent_table = caller
        .data()
        .linker_state()
        .os_table
        .expect("FATAL: OS Table not injected.");

    // MMU: Dynamic Allocation or Slab Recycling
    let recycled_slab = caller.data_mut().linker_state_mut().free_mem_slabs.pop();
    let (mem_base, mem_pages) = if let Some(slab) = recycled_slab {
        crate::kprintln!(
            "-> [ORE-LD] Reusing recycled memory slab at base: {}",
            slab.0
        );
        // Wipe the previous plugin's data so it can't leak into the new one,
        // and so zero-initialized statics (.bss) really start at zero.
        let start = slab.0 as usize;
        let end = start + (slab.1 as usize) * 65536;
        match memory.data_mut(&mut caller).get_mut(start..end) {
            Some(bytes) => bytes.fill(0),
            None => {
                crate::kprintln!(
                    "-> [ORE-LD] FATAL: Recycled slab at base {} is out of bounds.",
                    slab.0
                );
                set_err(
                    &mut caller,
                    format!("Recycled slab at base {} is out of bounds.", slab.0),
                );
                return -1;
            }
        }
        slab
    } else {
        // OS HEURISTIC: Give the plugin 16 Pages (1 Megabyte) of isolated data segment space.
        match allocate_plugin_memory(&mut caller, &memory, 16) {
            Ok(base) => (base, 16u64),
            Err(e) => {
                crate::kprintln!("-> [ORE-LD] FATAL: MMU Error: {}", e);
                set_err(&mut caller, format!("MMU Error: {}", e));
                return -1;
            }
        }
    };

    // We don't need to grow the function table at dlopen anymore, the OS pre-allocated 1024 slots for its own C-logic, Virtual Functions, and Callbacks.
    // We will dynamically grow the table later in `dlsym` on-demand
    let table_base = match agent_table.grow(&mut caller, 0, Ref::Func(None)) {
        Ok(tb) => tb as u32,
        Err(e) => {
            crate::kprintln!("-> [ORE-LD] FATAL: Failed to grow function table: {}", e);
            set_err(&mut caller, format!("Failed to grow function table: {}", e));
            release_slab(&mut caller, mem_base, mem_pages);
            return -1;
        }
    };

    // Forge the WebAssembly Globals required by the LLVM -fPIC ABI
    let (mem_global, table_global) = match forge_pic_globals(&mut caller, mem_base, table_base) {
        Ok(globals) => globals,
        Err(e) => {
            crate::kprintln!("-> [ORE-LD] FATAL: MMU Error forging ABI globals: {}", e);
            set_err(&mut caller, format!("MMU Error forging ABI globals: {}", e));
            release_slab(&mut caller, mem_base, mem_pages);
            return -1;
        }
    };

    // Create the Fusion Linker
    let mut plugin_linker: Linker<T> = Linker::new(caller.engine());

    // Silence errors here if the plugin didn't explicitly request an export.
    // Some plugins might not need the table.
    // [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
    let _ = plugin_linker.define(&mut caller, "env", "memory", memory);
    let _ = plugin_linker.define(&mut caller, "env", "__indirect_function_table", agent_table);
    let _ = plugin_linker.define(&mut caller, "env", "__memory_base", mem_global);
    let _ = plugin_linker.define(&mut caller, "env", "__table_base", table_global);

    let plugin_instance = match plugin_linker.instantiate(&mut caller, &plugin_module) {
        Ok(inst) => inst,
        Err(e) => {
            crate::kprintln!(
                "-> [ORE-LD] FATAL: Failed to link plugin {}: {}",
                filename,
                e
            );
            set_err(
                &mut caller,
                format!("Failed to link plugin {}: {}", filename, e),
            );
            release_slab(&mut caller, mem_base, mem_pages);
            return -1;
        }
    };

    // Fire the C-ABI Constructors
    if let Some(init) = plugin_instance.get_func(&mut caller, "__wasm_call_ctors")
        && let Ok(typed_init) = init.typed::<(), ()>(&caller)
        && let Err(e) = typed_init.call(&mut caller, ())
    {
        crate::kprintln!(
            "-> [ORE-LD] WARN: __wasm_call_ctors failed in {}: {}",
            filename,
            e
        );
    }

    // Construct the full PluginMetadata struct
    let metadata = PluginMetadata {
        instance: plugin_instance,
        mem_base,
        mem_pages,
        table_indices: Vec::new(),
    };

    // Register Handle in the Kernel OS State
    let handle = caller.data().linker_state().next_handle;
    caller
        .data_mut()
        .linker_state_mut()
        .loaded_plugins
        .insert(handle, metadata);
    caller.data_mut().linker_state_mut().next_handle += 1;

    crate::kprintln!(
        "-> [ORE-LD] Successfully fused '{}' into RAM at Memory Base: {}",
        filename,
        mem_base
    );

    handle
}

// [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
/// SYSCALL: ore_dlsym
/// Returns the integer index of the requested function, dynamically growing the table if necessary.
/// Returns: Function Pointer (Index > 0) on success, or 0 (NULL) on failure.
pub fn trap_ore_dlsym<T: HasLinkerState>(
    mut caller: Caller<'_, T>,
    handle: i32,
    symbol_ptr: u32,
    symbol_len: u32,
) -> i32 {
    // Safely extract Memory to read Symbol Name
    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => {
            set_err(
                &mut caller,
                "Host Agent did not export 'memory'.".to_string(),
            );
            return 0; // C-ABI expects 0 for NULL
        }
    };

    // SCOPE BLOCK: Confine the immutable borrow of `caller`
    let func_name = {
        let data = memory.data(&caller);
        let start = symbol_ptr as usize;
        let end = start.checked_add(symbol_len as usize).unwrap_or(0);

        if end > data.len() {
            set_err(
                &mut caller,
                "Memory bounds violation reading dlsym symbol name.".to_string(),
            );
            return 0;
        }

        match std::str::from_utf8(&data[start..end]) {
            Ok(s) => s.to_string(),
            Err(_) => {
                set_err(
                    &mut caller,
                    "Invalid UTF-8 in dlsym symbol name.".to_string(),
                );
                return 0;
            }
        }
    };

    // Copy out the instance handle (wasmtime::Instance implements Copy)
    let plugin_instance = match caller.data().linker_state().loaded_plugins.get(&handle) {
        Some(p) => p.instance,
        None => {
            crate::kprintln!(
                "-> [ORE-LD] ERROR: Invalid handle '{}' provided to dlsym.",
                handle
            );
            set_err(
                &mut caller,
                format!("Invalid handle '{}' provided to dlsym.", handle),
            );
            return 0; // NULL
        }
    };

    // Extract the requested Function from the instance
    let func = match plugin_instance.get_func(&mut caller, &func_name) {
        Some(f) => f,
        None => {
            crate::kprintln!(
                "-> [ORE-LD] ERROR: Symbol '{}' not found in plugin.",
                func_name
            );
            set_err(
                &mut caller,
                format!("Symbol '{}' not found in plugin.", func_name),
            );
            return 0; // NULL
        }
    };

    // Extract the Host's Function Table
    let agent_table = caller
        .data()
        .linker_state()
        .os_table
        .expect("FATAL: OS Table not injected.");

    // Dynamically expand the Host's routing table!
    let current_table_size = agent_table.size(&caller);

    // Grow the table by 1 slot. Wasmtime fills it with 'None' initially.
    if let Err(e) = agent_table.grow(&mut caller, 1, Ref::Func(None)) {
        crate::kprintln!("-> [ORE-LD] FATAL: Failed to grow table for dlsym: {}", e);
        set_err(
            &mut caller,
            format!("Failed to grow table for dlsym: {}", e),
        );
        return 0; // NULL
    }

    // Overwrite that new slot with the Plugin's actual function pointer!
    if let Err(e) = agent_table.set(&mut caller, current_table_size, Ref::Func(Some(func))) {
        crate::kprintln!(
            "-> [ORE-LD] FATAL: Failed to map table entry for dlsym: {}",
            e
        );
        set_err(
            &mut caller,
            format!("Failed to map table entry for dlsym: {}", e),
        );
        return 0; // NULL
    }

    // Record the table index in metadata for table poisoning on dlclose
    if let Some(meta) = caller
        .data_mut()
        .linker_state_mut()
        .loaded_plugins
        .get_mut(&handle)
    {
        meta.table_indices.push(current_table_size as u32);
    }

    crate::kprintln!(
        "-> [ORE-LD] Mapped symbol '{}' to Table Index {}",
        func_name,
        current_table_size
    );

    // Return the integer index to the Agent.
    current_table_size as i32
}

// [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
/// SYSCALL: ore_dlclose
/// Invalidate function pointers, free the handle, and recycle the memory slab.
/// Returns: 0 on success, -1 on failure.
pub fn trap_ore_dlclose<T: HasLinkerState>(mut caller: Caller<'_, T>, handle: i32) -> i32 {
    let (meta, table_opt) = {
        let state = caller.data_mut().linker_state_mut();

        let meta = match state.loaded_plugins.remove(&handle) {
            Some(m) => m,
            None => {
                crate::kprintln!(
                    "-> [ORE-LD] ERROR: Invalid handle '{}' passed to ore_dlclose.",
                    handle
                );
                state.last_error = Some(format!(
                    "Invalid handle '{}' passed to ore_dlclose.",
                    handle
                ));
                return -1;
            }
        };

        // Recycle the memory slab so future dlopen calls don't keep growing memory!
        state.free_mem_slabs.push((meta.mem_base, meta.mem_pages));
        let table = state.os_table;
        (meta, table)
    }; // The borrow of `caller` for `state` drops here!

    // Pass `idx as u64` to satisfy Wasmtime's Table::set API
    if let Some(table) = table_opt {
        for idx in meta.table_indices {
            let _ = table.set(&mut caller, idx as u64, Ref::Func(None));
        }
    }

    crate::kprintln!(
        "-> [ORE-LD] Successfully closed handle {} and poisoned its routing slots.",
        handle
    );
    0
}

// [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
/// SYSCALL: ore_dlerror
/// Copies the last error message into the guest's buffer.
/// Returns: Number of bytes written, or 0 if no error.
pub fn trap_ore_dlerror<T: HasLinkerState>(
    mut caller: Caller<'_, T>,
    buf_ptr: u32,
    buf_len: u32,
) -> i32 {
    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => return 0,
    };

    // Peek first: only clear the error once it has actually reached the guest.
    let error_msg = match caller.data().linker_state().last_error.clone() {
        Some(msg) => msg,
        None => return 0, // No error to report
    };

    let bytes = error_msg.as_bytes();
    let to_write = std::cmp::min(bytes.len(), buf_len as usize);

    if memory
        .write(&mut caller, buf_ptr as usize, &bytes[..to_write])
        .is_ok()
    {
        caller.data_mut().linker_state_mut().last_error = None;
        to_write as i32
    } else {
        -1
    }
}
