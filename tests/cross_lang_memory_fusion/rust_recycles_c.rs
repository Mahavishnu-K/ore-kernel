use ore_sys::host::Plugin;
use ore_sys::ore_bind;

// Loads and unloads the C `counter.wasi.so` plugin five times.
// Each round, `Plugin` is dropped at the end of the loop body, which calls ore_dlclose.
// If slab recycling works, every round reuses the same 1 MiB slab: the plugin's state
// address stays the same, the counter restarts at 1, and linear memory stops growing.
fn main() {
    println!("[Rust Host] Booting... I will load and unload a C plugin 5 times.");
    let mut failures = 0;

    let mut first_addr: Option<*const i32> = None;
    let mut pages_after_round_1 = 0;

    for round in 1..=5 {
        let plugin = Plugin::load("counter.wasi.so").expect("Build counter.c with --shared first!");
        ore_bind!(plugin, counter_next, fn() -> i32);
        ore_bind!(plugin, counter_state_addr, fn() -> *const i32);

        let a = counter_next();
        let b = counter_next();
        let addr = counter_state_addr();
        let pages = core::arch::wasm32::memory_size::<0>();
        println!(
            "[Rust Host] Round {}: counter = {}, {} | state at {:p} | memory = {} pages",
            round, a, b, addr, pages
        );

        if a != 1 || b != 2 {
            println!("[Rust Host] FAIL round {} did not start from fresh plugin state", round);
            failures += 1;
        }

        match first_addr {
            None => {
                first_addr = Some(addr);
                pages_after_round_1 = pages;
            }
            Some(first) => {
                if addr != first {
                    println!("[Rust Host] FAIL round {} got a new slab instead of the recycled one", round);
                    failures += 1;
                }
                // A new plugin slab is 16 pages; the host's own heap may grow a little.
                if pages >= pages_after_round_1 + 16 {
                    println!("[Rust Host] FAIL round {} grew linear memory by a whole slab", round);
                    failures += 1;
                }
            }
        }

        // `plugin` is dropped here -> ore_dlclose -> slab goes back on the free list.
    }

    if failures == 0 {
        println!("[Rust Host] ALL CHECKS PASSED (5 loads, 1 slab)");
    } else {
        println!("[Rust Host] {} CHECK(S) FAILED", failures);
        std::process::exit(1);
    }
}
