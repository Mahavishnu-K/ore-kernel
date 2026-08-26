#include <stdio.h>
#include <ore.h>

int main() {
    printf("[C Host] Booting... I will use a Rust plugin to encrypt my memory.\n");

    OrePlugin plugin = ore_load("encryptor.wasi.so");
    if (plugin <= 0) {
        printf("[C Host FATAL] Failed to load Rust plugin.\n");
        return 1;
    }

    // Bind the Rust function! (Note: usize maps to int/unsigned int, u8 maps to unsigned char)
    ORE_BIND(plugin, xor_cipher, void, char*, int, unsigned char);

    char secret_data[] = "C_LOVES_RUST_MEMORY_FUSION";
    int len = __ore_strlen(secret_data);

    printf("[C Host] Original: %s\n", secret_data);

    // Call the Rust logic natively!
    xor_cipher(secret_data, len, 0xAA);
    printf("[C Host] Rust Encrypted: ");
    for(int i = 0; i < len; i++) printf("%02x ", (unsigned char)secret_data[i]);
    printf("\n");

    // Decrypt
    xor_cipher(secret_data, len, 0xAA);
    printf("[C Host] Rust Decrypted: %s\n", secret_data);

    return 0;
}