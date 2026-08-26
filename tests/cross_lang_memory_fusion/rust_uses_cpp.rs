use ore_sys::host::Plugin;
use ore_sys::ore_bind;

fn main() {
    println!("[Rust Host] Booting... I will use a C++ plugin to clean my memory.");

    let plugin = Plugin::load("sanitizer.wasi.so").expect("C++ Plugin missing!");

    // Bind the C++ function!
    ore_bind!(plugin, sanitize_spaces, fn(*mut u8, i32) -> i32);

    let mut messy_data = String::from("Rust     fuses    with     C++     flawlessly!").into_bytes();
    println!("[Rust Host] Original: '{}'", String::from_utf8_lossy(&messy_data));

    // Call the C++ logic natively!
    let new_len = unsafe { sanitize_spaces(messy_data.as_mut_ptr(), messy_data.len() as i32) };
    
    // Truncate the Rust vector to the new length returned by C++
    messy_data.truncate(new_len as usize);

    println!("[Rust Host] C++ Cleaned: '{}'", String::from_utf8_lossy(&messy_data));
}