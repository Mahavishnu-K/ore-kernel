// Because Plugins cannot use the Standard Library (to prevent malloc collisions),
// Rust requires a manual panic handler. We provide a safe, silent infinite loop.
// The ORE Kernel's Fuel Limit (5 Billion instructions) will catch this loop
// and safely terminate the sandbox if the plugin crashes
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// The ORE Plugin Macro
/// Automatically converts a standard Rust function into a C-ABI compliant WebAssembly export
#[macro_export]
macro_rules! ore_export {
    (
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? {
            $($body:tt)*
        }
    ) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name($($arg : $ty),*) $(-> $ret)? {
            $($body)*
        }
    };
}
