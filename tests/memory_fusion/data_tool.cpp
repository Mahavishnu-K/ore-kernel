#include <iostream>
#include <string>
#include <ore.h>

int main() {
    std::cout << "[C++ Agent] Data Cleaner Booting..." << std::endl;

    OrePlugin plugin = ore_load("sanitizer.wasi.so");
    if (plugin <= 0) {
        std::cout << "Failed to load plugin." << std::endl;
        return 1;
    }

    // No typedefs needed anymore!
    ORE_BIND(plugin, sanitize_spaces, int, char*, int);

    // A messy string from a web scraper
    std::string raw_data = "This    is   a    very    messy     dataset.";
    std::cout << "[C++ Agent] Raw: '" << raw_data << "'" << std::endl;

    // Mutate the C++ std::string's internal buffer directly!
    int new_len = sanitize_spaces(&raw_data[0], raw_data.length());
    raw_data.resize(new_len);

    std::cout << "[C++ Agent] Cleaned: '" << raw_data << "'" << std::endl;

    return 0;
}