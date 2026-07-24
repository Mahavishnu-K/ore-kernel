use std::str;

// ---------------------------------------------------------
// THE ORE-SDK (This is what ore will publish to crates.io!)
// ---------------------------------------------------------
#[link(wasm_import_module = "ore")]
extern "C" {
    fn fetch(
        method_ptr: *const u8, method_len: usize,
        url_ptr: *const u8, url_len: usize,
        body_ptr: *const u8, body_len: usize,
        filename_ptr: *const u8, filename_len: usize,
    ) -> i32;
}

/// The beautiful, developer-friendly wrapper
fn ore_fetch(method: &str, url: &str, body: &str, filename: &str) -> Result<String, i32> {
    let res = unsafe {
        fetch(
            method.as_ptr(), method.len(),
            url.as_ptr(), url.len(),
            body.as_ptr(), body.len(),
            filename.as_ptr(), filename.len(),
        )
    };

    if res == 0 {
        // The Kernel streamed it to the VFS. Let's read it!
        let path = format!("/ore_tmp/{}", filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            Ok(content)
        } else {
            Err(-4) // File read error
        }
    } else {
        Err(res) // Firewall blocked it!
    }
}

// ---------------------------------------------------------
// THE AGENT TOOL LOGIC (Translated from your JS)
// ---------------------------------------------------------
fn main() {
    println!("Starting network interception test in Rust/WASM...");

    // Test 1: Allowed Domain & Method
    println!("\n[*] Attempting to fetch from jsonplaceholder.typicode.com (Allowed)...");
    match ore_fetch("GET", "https://jsonplaceholder.typicode.com/todos/1", "", "test1.json") {
        Ok(data) => println!("SUCCESS: Allowed domain fetch worked.\nResult: {}", data),
        Err(e) => println!("ERROR: Allowed domain fetch failed. Code: {}", e),
    }

    // Test 2: Blocked Domain
    println!("\n[*] Attempting to fetch from google.com (Should be Blocked)...");
    match ore_fetch("GET", "https://www.google.com", "", "test2.html") {
        Ok(_) => println!("VULNERABILITY: Blocked domain fetch succeeded! The firewall failed."),
        Err(e) => println!("SUCCESS: Blocked domain fetch was intercepted and denied. Code: {}", e),
    }

    // Test 3: Blocked HTTP Method
    println!("\n[*] Attempting a POST request to jsonplaceholder.typicode.com (Should be Blocked)...");
    let body = r#"{"title": "foo", "body": "bar", "userId": 1}"#;
    match ore_fetch("POST", "https://jsonplaceholder.typicode.com/posts", body, "test3.json") {
        Ok(_) => println!("VULNERABILITY: POST request succeeded! The method firewall failed."),
        Err(e) => println!("SUCCESS: POST request was intercepted and denied due to method rules. Code: {}", e),
    }
}