use crate::linker::linker_state::LinkerState;
use crate::linker::mmu::{allocate_plugin_memory, forge_pic_globals};
use wasmtime::{Caller, Linker, Module, Ref};

/// Trait to ensure the Caller's generic State contains our Linker Registry.
/// The master Sandbox state struct in sandbox.rs must implement this!
pub trait HasLinkerState {
    fn linker_state(&self) -> &LinkerState;
    fn linker_state_mut(&mut self) -> &mut LinkerState;
}

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
            return -1;
        }

        match std::str::from_utf8(&data[start..end]) {
            Ok(s) => s.to_string(), // THE FIX: Copy into an Owned String!
            Err(_) => {
                crate::kprintln!("-> [ORE-LD] FATAL: Invalid UTF-8 in dlopen filename.");
                return -1;
            }
        }
    }; // The `data` reference is dropped here. `caller` is now completely unlocked!

    // File Security & I/O
    let plugin_path = crate::get_ore_dir().join("plugins").join(&filename);
    if !plugin_path.exists() {
        crate::kprintln!(
            "-> [ORE-LD] ERROR: Plugin '{}' not found in ~/.ore/plugins/",
            filename
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
            return -1;
        }
    };

    // Extract the Agent's Function Table (The Routing Brain)
    let agent_table = caller
        .data()
        .linker_state()
        .os_table
        .expect("FATAL: OS Table not injected.");

    // MMU: Dynamic Allocation for the Plugin
    // OS HEURISTIC: Give the plugin 16 Pages (1 Megabyte) of isolated data segment space.
    let mem_base = match allocate_plugin_memory(&mut caller, &memory, 16) {
        Ok(base) => base,
        Err(e) => {
            crate::kprintln!("-> [ORE-LD] FATAL: MMU Error: {}", e);
            return -1;
        }
    };

    // We don't need to grow the function table at dlopen anymore, the OS pre-allocated 1024 slots for its own C-logic, Virtual Functions, and Callbacks.
    // We will dynamically grow the table later in `dlsym` on-demand!
    let table_base = match agent_table.grow(&mut caller, 0, Ref::Func(None)) {
        Ok(tb) => tb as u32,
        Err(e) => {
            crate::kprintln!("-> [ORE-LD] FATAL: Failed to grow function table: {}", e);
            return -1;
        }
    };

    // Forge the WebAssembly Globals required by the LLVM -fPIC ABI
    let (mem_global, table_global) = match forge_pic_globals(&mut caller, mem_base, table_base) {
        Ok(globals) => globals,
        Err(e) => {
            crate::kprintln!("-> [ORE-LD] FATAL: MMU Error forging ABI globals: {}", e);
            return -1;
        }
    };

    // Create the Fusion Linker
    let mut plugin_linker: Linker<T> = Linker::new(caller.engine());

    // Silence errors here if the plugin didn't explicitly request an export.
    // Some plugins might not need the table.
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
            return -1;
        }
    };

    // Fire the C-ABI Constructors
    // The plugin must initialize its global variables into the shared RAM
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

    // Register Handle in the Kernel OS State
    let handle = caller.data().linker_state().next_handle;
    caller
        .data_mut()
        .linker_state_mut()
        .loaded_plugins
        .insert(handle, plugin_instance);
    caller.data_mut().linker_state_mut().next_handle += 1;

    crate::kprintln!(
        "-> [ORE-LD] Successfully fused '{}' into RAM at Memory Base: {}",
        filename,
        mem_base
    );

    handle
}

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
        None => return 0, // C-ABI expects 0 for NULL
    };

    // SCOPE BLOCK: Confine the immutable borrow of `caller`
    let func_name = {
        let data = memory.data(&caller);
        let start = symbol_ptr as usize;
        let end = start.checked_add(symbol_len as usize).unwrap_or(0);

        if end > data.len() {
            return 0;
        }

        match std::str::from_utf8(&data[start..end]) {
            Ok(s) => s.to_string(), // THE FIX: Copy into an Owned String!
            Err(_) => return 0,
        }
    }; // Immutable borrow of `caller` drops here!

    // Fetch the Plugin from the Registry
    let plugin = match caller.data().linker_state().loaded_plugins.get(&handle) {
        Some(p) => *p,
        None => {
            crate::kprintln!(
                "-> [ORE-LD] ERROR: Invalid handle '{}' provided to dlsym.",
                handle
            );
            return 0; // NULL
        }
    };

    // Extract the requested Function
    let func = match plugin.get_func(&mut caller, &func_name) {
        Some(f) => f,
        None => {
            crate::kprintln!(
                "-> [ORE-LD] ERROR: Symbol '{}' not found in plugin.",
                func_name
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
        return 0; // NULL
    }

    // Overwrite that new slot with the Plugin's actual function pointer!
    if let Err(e) = agent_table.set(&mut caller, current_table_size, Ref::Func(Some(func))) {
        crate::kprintln!(
            "-> [ORE-LD] FATAL: Failed to map table entry for dlsym: {}",
            e
        );
        return 0; // NULL
    }

    crate::kprintln!(
        "-> [ORE-LD] Mapped symbol '{}' to Table Index {}",
        func_name,
        current_table_size
    );

    // Return the integer index to the Agent.
    current_table_size as i32
}
