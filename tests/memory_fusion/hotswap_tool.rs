use ore_sys::host::Plugin;
use ore_sys::ore_bind;

fn check(failures: &mut u32, ok: bool, what: &str) {
    println!("[Rust Agent] {} {}", if ok { "PASS" } else { "FAIL" }, what);
    if !ok {
        *failures += 1;
    }
}

fn main() {
    println!("[Rust Agent] Hot-swap test booting...");
    let mut failures = 0;

    // State that must survive the swap: it lives in the host's memory, not the plugin's.
    let order_book: [u32; 3] = [100, 250, 75];
    let mut history: Vec<(u32, u32)> = Vec::new(); // (model version, quote)

    // 1. A failed load is reported through Plugin::last_error().
    let missing = Plugin::load("pricing_v3.wasi.so");
    check(&mut failures, missing.is_err(), "loading a missing plugin fails");
    let err = Plugin::last_error();
    println!("[Rust Agent] last_error = {:?}", err);
    check(&mut failures, err.is_some(), "last_error explains the failed load");

    // 2. Run on v1, then close it explicitly.
    {
        let v1 = Plugin::load("pricing_v1.wasi.so").expect("Build pricing_v1.rs with --shared first!");
        ore_bind!(v1, quote, fn(u32) -> u32);
        ore_bind!(v1, model_version, fn() -> u32);

        let version = model_version();
        let total: u32 = order_book.iter().map(|&base| quote(base)).sum();
        for &base in &order_book {
            history.push((version, quote(base)));
        }
        println!("[Rust Agent] Model v{} total = {}", version, total);
        check(&mut failures, version == 1 && total == 850, "v1 quotes the order book at 850");

        // A missing symbol is an Err, and the kernel says why.
        let bad = v1.get_func::<extern "C" fn() -> u32>("no_such_symbol");
        check(&mut failures, bad.is_err(), "binding a missing symbol fails");
        let err = Plugin::last_error();
        println!("[Rust Agent] last_error = {:?}", err);
        check(&mut failures, err.is_some(), "last_error explains the failed bind");

        check(&mut failures, v1.close().is_ok(), "explicit close() of v1 succeeds");
    }

    // 3. Swap in v2. The host's history is untouched.
    {
        let v2 = Plugin::load("pricing_v2.wasi.so").expect("Build pricing_v2.rs with --shared first!");
        ore_bind!(v2, quote, fn(u32) -> u32);
        ore_bind!(v2, model_version, fn() -> u32);

        let version = model_version();
        let total: u32 = order_book.iter().map(|&base| quote(base)).sum();
        for &base in &order_book {
            history.push((version, quote(base)));
        }
        println!("[Rust Agent] Model v{} total = {}", version, total);
        check(&mut failures, version == 2 && total == 1275, "v2 quotes the order book at 1275");

        // v2 is closed automatically here, when it goes out of scope (Drop).
    }

    println!("[Rust Agent] History across the swap: {:?}", history);
    check(
        &mut failures,
        history == [(1, 200), (1, 500), (1, 150), (2, 300), (2, 750), (2, 225)],
        "host state survived the swap",
    );

    if failures == 0 {
        println!("[Rust Agent] ALL CHECKS PASSED");
    } else {
        println!("[Rust Agent] {} CHECK(S) FAILED", failures);
        std::process::exit(1);
    }
}
