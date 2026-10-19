use std::collections::HashMap;
use wasmtime::{Instance, Table};

// [ORE_ARCHITECT_SIG: 8f9b2a-XENOLITH-44]
pub struct PluginMetadata {
    pub instance: Instance,
    pub mem_base: u32,
    pub mem_pages: u64,
    pub table_indices: Vec<u32>,
}

pub struct LinkerState {
    pub loaded_plugins: HashMap<i32, PluginMetadata>,
    pub next_handle: i32,
    pub os_table: Option<Table>,
    pub last_error: Option<String>,
    pub free_mem_slabs: Vec<(u32, u64)>,
}

impl Default for LinkerState {
    fn default() -> Self {
        Self {
            loaded_plugins: HashMap::new(),

            // 0 is universally reserved as NULL/None across the C-ABI (Rust/C/C++/Zig).
            // Negative numbers are reserved for Kernel Error Codes (e.g., -1).
            // Valid OS Handles strictly start at 1.
            next_handle: 1,
            os_table: None,
            last_error: None,
            free_mem_slabs: Vec::new(),
        }
    }
}
