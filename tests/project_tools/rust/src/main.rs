mod formatter;
use serde::Deserialize;
use std::io::{self, Read};

#[derive(Deserialize)]
struct Input {
    markdown: String,
}

fn main() {
    let mut input_data = String::new();
    io::stdin().read_to_string(&mut input_data).unwrap();

    let payload: Input = serde_json::from_str(&input_data).expect("Invalid JSON");
    
    let html = formatter::convert_to_html(&payload.markdown);
    println!("SUCCESS (Rust): Generated HTML:\n{}", html);
}