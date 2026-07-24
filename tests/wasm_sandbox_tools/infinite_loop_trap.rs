fn main() {
    println!("Starting infinite loop fuel test in Rust...");
    let mut counter: u64 = 0;
    loop {
        counter += 1;
        if counter % 10_000_000 == 0 {
            println!("Rust still looping... {}", counter);
        }
        // This will eventually trigger the 50-million instruction limit in wasmtime
    }
}
