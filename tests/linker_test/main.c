// tests/linker_test/main.c
#include <stdio.h>
#include <string.h>

// The ORE OS Syscall Signatures
__attribute__((import_module("env"), import_name("ore_dlopen")))
int ore_dlopen(const char* filename, unsigned int len);

__attribute__((import_module("env"), import_name("ore_dlsym")))
int ore_dlsym(int handle, const char* symbol, unsigned int len);

// The function signature we expect from the plugin
typedef int (*calc_func)(int, int);

int main() {
    printf("[Agent] Booting up inside ORE Sandbox...\n");

    const char* plugin_name = "plugin.wasi.so";
    printf("[Agent] Trapping to Kernel: dlopen('%s')\n", plugin_name);
    
    // Call the Kernel, passing the string and its length
    int handle = ore_dlopen(plugin_name, strlen(plugin_name));
    
    if (handle <= 0) {
        printf("[Agent FATAL] Kernel refused to load plugin!\n");
        return 1;
    }
    printf("[Agent] Kernel returned Handle ID: %d\n", handle);

    const char* func_name = "calculate";
    printf("[Agent] Trapping to Kernel: dlsym('%s')\n", func_name);
    
    // Call the Kernel to expand the table and get the pointer
    int func_idx = ore_dlsym(handle, func_name, strlen(func_name));

    if (func_idx <= 0) {
        printf("[Agent FATAL] Kernel failed to find symbol!\n");
        return 1;
    }
    printf("[Agent] Kernel expanded routing table. Symbol mapped to Index: %d\n", func_idx);

    // Cast the integer index into a WebAssembly Function Pointer
    calc_func calculate = (calc_func)func_idx;

    printf("[Agent] Executing dynamically linked function...\n");
    int result = calculate(10, 32);

    printf("[Agent SUCCESS] The result of 10 + 32 is: %d\n", result);

    return 0;
}