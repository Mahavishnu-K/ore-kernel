#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn ore_dlopen(filename_ptr: *const u8, filename_len: u32) -> i32;
    fn ore_dlsym(handle: i32, symbol_ptr: *const u8, symbol_len: u32) -> i32;
    fn ore_dlclose(handle: i32) -> i32;
    fn ore_dlerror(buf_ptr: *mut u8, buf_len: u32) -> i32;
}

pub struct Plugin {
    handle: i32,
}

impl Plugin {
    /// Loads a .wasi.so plugin into the Host Agent's RAM natively.
    pub fn load(plugin_name: &str) -> Result<Self, &'static str> {
        let handle = unsafe { ore_dlopen(plugin_name.as_ptr(), plugin_name.len() as u32) };

        if handle <= 0 {
            return Err("Failed to load plugin. Check Kernel logs.");
        }

        Ok(Self { handle })
    }

    /// Extracts a function pointer from the plugin.
    /// `T` must be the C-ABI function signature (e.g., `extern "C" fn(i32) -> i32`).
    pub fn get_func<T>(&self, symbol: &str) -> Result<T, &'static str> {
        let func_idx = unsafe { ore_dlsym(self.handle, symbol.as_ptr(), symbol.len() as u32) };

        if func_idx <= 0 {
            return Err("Symbol not found in plugin.");
        }

        // SAFETY: In WebAssembly, a C-function pointer is just a usize index
        // in the routing table. We cast the index directly into the function signature
        unsafe {
            let ptr_val = func_idx as usize;
            Ok(core::mem::transmute_copy(&ptr_val))
        }
    }

    /// Closes the plugin explicitly and reports the result.
    /// Dropping a `Plugin` also closes it; use this when you need to know whether it succeeded.
    /// Function pointers bound from this plugin trap if called afterwards.
    pub fn close(mut self) -> Result<(), &'static str> {
        let res = unsafe { ore_dlclose(self.handle) };
        // Mark as closed either way so `Drop` doesn't try again.
        self.handle = 0;
        if res == 0 {
            Ok(())
        } else {
            Err("Failed to close plugin")
        }
    }

    /// Returns the kernel's last linker error, if any, and clears it.
    pub fn last_error() -> Option<String> {
        let mut buf = [0u8; 256];
        let bytes = unsafe { ore_dlerror(buf.as_mut_ptr(), buf.len() as u32) };
        if bytes > 0 {
            Some(String::from_utf8_lossy(&buf[..bytes as usize]).into_owned())
        } else {
            None
        }
    }
}

// RAII: a plugin closes itself when it goes out of scope.
impl Drop for Plugin {
    fn drop(&mut self) {
        if self.handle > 0 {
            unsafe {
                ore_dlclose(self.handle);
            }
        }
    }
}

/// The ORE Bind Macro
/// Automatically extracts a C-ABI function pointer from a loaded plugin,
/// casts it safely, and binds it to a local variable of the same name
#[macro_export]
macro_rules! ore_bind {
    (
        $plugin:expr,
        $func_name:ident,
        fn($($arg_ty:ty),*) $(-> $ret_ty:ty)?
    ) => {
        let $func_name = $plugin.get_func::<extern "C" fn($($arg_ty),*) $(-> $ret_ty)?>(stringify!($func_name))
            .expect(concat!("ORE FATAL: Failed to resolve symbol '", stringify!($func_name), "' in plugin."));
    };
}
