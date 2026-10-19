#include <stdio.h>
#include <ore.h>

static int failures = 0;

static void check(int ok, const char* what) {
    printf("[C Agent] %s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok) failures++;
}

// Prints the kernel's last linker error (ore_dlerror). Returns 1 if there was one.
static int print_last_error(const char* context) {
    char buf[256];
    int n = ore_error(buf, sizeof(buf) - 1);
    if (n > 0) {
        buf[n] = '\0';
        printf("[C Agent] %s -> ore_error: \"%s\"\n", context, buf);
        return 1;
    }
    printf("[C Agent] %s -> ore_error: (none)\n", context);
    return 0;
}

int main() {
    printf("[C Agent] ore-ld lifecycle test booting...\n");

    // 1. A failed dlopen reports why, and the error is cleared once read.
    OrePlugin missing = ore_load("does_not_exist.wasi.so");
    check(missing <= 0, "loading a missing plugin fails");
    check(print_last_error("after failed load"), "ore_error explains the failed load");
    check(!print_last_error("reading again"), "ore_error is cleared after it is read");

    // 2. Load the plugin and use it.
    OrePlugin plugin = ore_load("counter.wasi.so");
    if (plugin <= 0) {
        print_last_error("loading counter.wasi.so");
        printf("[C Agent] FATAL: build counter.c with `ore mktool counter.c --shared` first.\n");
        return 1;
    }
    printf("[C Agent] Loaded counter.wasi.so as handle %d\n", plugin);

    int* first_addr;
    {
        ORE_BIND(plugin, counter_next, int, void);
        ORE_BIND(plugin, counter_state_addr, int*, void);

        int a = counter_next();
        int b = counter_next();
        int c = counter_next();
        printf("[C Agent] counter_next() x3 = %d, %d, %d\n", a, b, c);
        check(a == 1 && b == 2 && c == 3, "plugin state counts 1, 2, 3");

        first_addr = counter_state_addr();
        printf("[C Agent] Plugin state lives at %p\n", (void*)first_addr);
    }

    // 3. A missing symbol returns NULL and reports why.
    {
        ORE_BIND(plugin, no_such_symbol, int, void);
        check(no_such_symbol == 0, "binding a missing symbol returns NULL");
        check(print_last_error("after failed bind"), "ore_error explains the failed bind");
    }

    // 4. Close it, then close it again.
    check(ore_close(plugin) == 0, "ore_close succeeds");
    check(ore_close(plugin) != 0, "closing the same handle twice fails");
    check(print_last_error("after double close"), "ore_error explains the double close");

    // 5. Reload: the kernel should reuse the freed slab, wiped clean.
    OrePlugin reloaded = ore_load("counter.wasi.so");
    if (reloaded <= 0) {
        print_last_error("reloading counter.wasi.so");
        return 1;
    }
    printf("[C Agent] Reloaded counter.wasi.so as handle %d\n", reloaded);
    check(reloaded != plugin, "reload gets a fresh handle");

    {
        ORE_BIND(reloaded, counter_next, int, void);
        ORE_BIND(reloaded, counter_state_addr, int*, void);

        int* second_addr = counter_state_addr();
        printf("[C Agent] Reloaded state lives at %p\n", (void*)second_addr);
        check(second_addr == first_addr, "reload reuses the same memory slab");

        int first = counter_next();
        printf("[C Agent] counter_next() after reload = %d\n", first);
        check(first == 1, "recycled slab was zeroed (counter restarts at 1, not 4)");
    }

    check(ore_close(reloaded) == 0, "closing the reloaded plugin succeeds");

    if (failures == 0) {
        printf("[C Agent] ALL CHECKS PASSED\n");
        return 0;
    }
    printf("[C Agent] %d CHECK(S) FAILED\n", failures);
    return 1;
}
