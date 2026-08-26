#![no_std]
use ore_sys::ore_export;

// A high-speed, in-place XOR encryptor/decryptor.
// Notice it doesn't return anything. It mutates the host's RAM directly.
ore_export! {
    fn xor_cipher(data_ptr: *mut u8, len: usize, key: u8) {
        // Reconstruct the slice from the Host's RAM pointer
        let slice = unsafe { core::slice::from_raw_parts_mut(data_ptr, len) };
        
        for byte in slice.iter_mut() {
            *byte ^= key;
        }
    }
}