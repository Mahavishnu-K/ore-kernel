#include <iostream>
#include <vector>
#include <ore.h>

// A C++ host hot-swaps a Rust pricing plugin (v1 -> v2) without losing its own state.
static int failures = 0;

static void check(bool ok, const char* what) {
    std::cout << "[C++ Host] " << (ok ? "PASS " : "FAIL ") << what << std::endl;
    if (!ok) failures++;
}

static bool print_last_error(const char* context) {
    char buf[256];
    int n = ore_error(buf, sizeof(buf) - 1);
    if (n > 0) {
        buf[n] = '\0';
        std::cout << "[C++ Host] " << context << " -> ore_error: \"" << buf << "\"" << std::endl;
        return true;
    }
    std::cout << "[C++ Host] " << context << " -> ore_error: (none)" << std::endl;
    return false;
}

int main() {
    std::cout << "[C++ Host] Booting... I will hot-swap a Rust plugin under live state." << std::endl;

    // State owned by the C++ host. It must survive the swap.
    std::vector<unsigned int> order_book = {100, 250, 75};
    std::vector<unsigned int> quotes;

    // 1. Load v1 and quote the book.
    OrePlugin v1 = ore_load("pricing_v1.wasi.so");
    if (v1 <= 0) {
        print_last_error("loading pricing_v1.wasi.so");
        return 1;
    }
    {
        ORE_BIND(v1, quote, unsigned int, unsigned int);
        ORE_BIND(v1, model_version, unsigned int, void);

        unsigned int total = 0;
        for (unsigned int base : order_book) {
            unsigned int q = quote(base);
            quotes.push_back(q);
            total += q;
        }
        std::cout << "[C++ Host] Rust model v" << model_version() << " total = " << total << std::endl;
        check(model_version() == 1 && total == 850, "v1 quotes the order book at 850");
    }

    // 2. Unload v1.
    check(ore_close(v1) == 0, "ore_close(v1) succeeds");

    // 3. A bad upgrade path reports why.
    OrePlugin v3 = ore_load("pricing_v3.wasi.so");
    check(v3 <= 0, "loading a missing pricing_v3 fails");
    check(print_last_error("after failed upgrade"), "ore_error explains the failed upgrade");

    // 4. Swap in v2 and quote again.
    OrePlugin v2 = ore_load("pricing_v2.wasi.so");
    if (v2 <= 0) {
        print_last_error("loading pricing_v2.wasi.so");
        return 1;
    }
    {
        ORE_BIND(v2, quote, unsigned int, unsigned int);
        ORE_BIND(v2, model_version, unsigned int, void);

        unsigned int total = 0;
        for (unsigned int base : order_book) {
            unsigned int q = quote(base);
            quotes.push_back(q);
            total += q;
        }
        std::cout << "[C++ Host] Rust model v" << model_version() << " total = " << total << std::endl;
        check(model_version() == 2 && total == 1275, "v2 quotes the order book at 1275");
    }
    check(ore_close(v2) == 0, "ore_close(v2) succeeds");

    // 5. The host's state spans both versions.
    std::vector<unsigned int> expected = {200, 500, 150, 300, 750, 225};
    check(quotes == expected, "host state survived the swap (6 quotes from 2 models)");

    if (failures == 0) {
        std::cout << "[C++ Host] ALL CHECKS PASSED" << std::endl;
        return 0;
    }
    std::cout << "[C++ Host] " << failures << " CHECK(S) FAILED" << std::endl;
    return 1;
}
