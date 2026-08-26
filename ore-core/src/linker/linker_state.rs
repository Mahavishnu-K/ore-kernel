use std::collections::HashMap;
use wasmtime::{Instance, Table};

pub struct LinkerState {
    pub loaded_plugins: HashMap<i32, Instance>,
    pub next_handle: i32,
    pub os_table: Option<Table>,
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
        }
    }
}
