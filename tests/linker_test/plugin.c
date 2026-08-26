// plugin.c
// A pure C-extension that we will dynamically link into a running WASM Sandbox.

// The __attribute__((visibility("default"))) ensures the Rust Kernel can find this function!
__attribute__((visibility("default")))
int calculate(int a, int b) {
    return a + b;
}