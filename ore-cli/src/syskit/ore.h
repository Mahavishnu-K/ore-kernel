// ore.h - The Official ORE Systems SDK

#ifndef ORE_H
#define ORE_H

#ifdef __cplusplus
extern "C" {
#endif

// PLUGIN API (Used by the Developer to export math to .wasi.so)
// Makes the function visible to the ORE Kernel for dynamic linking.
#ifdef __cplusplus
// If compiling C++, automatically inject extern "C" to prevent Name Mangling!
#define ORE_PLUGIN extern "C" __attribute__((visibility("default")))
#else
// Standard C compiler
#define ORE_PLUGIN __attribute__((visibility("default")))
#endif

// HOST API (Used by the Developer to load .wasi.so inside the Agent)
#ifndef ORE_PLUGIN_MODE

// Raw WebAssembly Imports (Hidden from the developer)
__attribute__((import_module("env"), import_name("ore_dlopen")))
extern int __ore_dlopen(const char* filename, unsigned int len);

__attribute__((import_module("env"), import_name("ore_dlsym")))
extern int __ore_dlsym(int handle, const char* symbol, unsigned int len);

__attribute__((import_module("env"), import_name("ore_dlclose")))
extern int __ore_dlclose(int handle);

__attribute__((import_module("env"), import_name("ore_dlerror")))
extern int __ore_dlerror(char* buffer, unsigned int max_len);

// A safe handle for a loaded ORE Plugin
typedef int OrePlugin;

// Tiny custom strlen so we don't force <string.h> dependencies on the developer
static inline unsigned int __ore_strlen(const char* str) {
    unsigned int len = 0;
    while (str[len] != '\0') len++;
    return len;
}

// Elegant API to load a plugin
static inline OrePlugin ore_load(const char* plugin_name) {
    return __ore_dlopen(plugin_name, __ore_strlen(plugin_name));
}

static inline int ore_close(OrePlugin plugin) {
    return __ore_dlclose(plugin);
}

static inline int ore_error(char* buffer, unsigned int max_len) {
    return __ore_dlerror(buffer, max_len);
}

// Macro to automatically cast the function pointer and bind it to a variable
#define ORE_BIND(plugin_handle, func_name, ret_type, ...) \
    ret_type (*func_name)(__VA_ARGS__) = (ret_type (*)(__VA_ARGS__))__ore_dlsym(plugin_handle, #func_name, __ore_strlen(#func_name))


#endif // ORE_PLUGIN_MODE

#ifdef __cplusplus
}
#endif

#endif // ORE_H