use std::io::{self, Read};
use std::time::Instant;

fn main() {
    let start_time = Instant::now();

    // 1. Test STDIN Pipe
    let mut input_data = String::new();
    io::stdin().read_to_string(&mut input_data).unwrap();

    // 2. Heavy Compute (Iterating over every byte)
    let mut vowel_count = 0;
    for c in input_data.chars() {
        if "aeiouAEIOU".contains(c) {
            vowel_count += 1;
        }
    }

    let compute_time = start_time.elapsed();

    // 3. Test STDOUT Pipe
    println!("--- RUST INTERNAL METRICS ---");
    println!("Processed bytes : {}", input_data.len());
    println!("Vowels found    : {}", vowel_count);
    println!("Compute latency : {:?}", compute_time);
}