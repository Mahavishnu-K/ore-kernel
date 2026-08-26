use ore_sys::host::Plugin;
use ore_sys::ore_bind;


fn main() {
    println!("[Rust Agent] Booting...");

    let plugin = Plugin::load("encryptor.wasi.so").expect("Failed to load plugin!");

    // 2. The God-Tier Macro: Automatically casts and binds the variable!
    ore_bind!(plugin, xor_cipher, fn(*mut u8, usize, u8));

    // The Payload (Living in the Host's RAM)
    let mut payload = String::from("CLASSIFIED_ENTERPRISE_DATA").into_bytes();
    println!("[Rust Agent] Original Data : {:?}", String::from_utf8_lossy(&payload));

    // 3. Zero-Copy Execution (Since it's an FFI C-function, we wrap in unsafe)
    unsafe { xor_cipher(payload.as_mut_ptr(), payload.len(), 0xAA) };
    println!("[Rust Agent] Encrypted     : {:?}", payload);

    // Call it again to decrypt (XOR is symmetric)
    unsafe { xor_cipher(payload.as_mut_ptr(), payload.len(), 0xAA) };
    println!("[Rust Agent] Decrypted     : {:?}", String::from_utf8_lossy(&payload));
}