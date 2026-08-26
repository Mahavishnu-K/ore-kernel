#include <stdio.h>
#include <ore.h>

int main() {
    printf("[C Agent] Financial Analysis starting...\n");

    // 1. Elegant plugin loading
    OrePlugin plugin = ore_load("stats.wasi.so");
    if (plugin <= 0) {
        printf("[C Agent FATAL] Failed to load plugin!\n");
        return 1;
    }
    
    // 2. The C Macro Magic (Handle, Func Name, Return Type, Args...)
    ORE_BIND(plugin, calculate_variance, double, const double*, int);

    // Simulate 10 days of volatile stock prices in the Host's RAM
    double market_data[] = {150.5, 152.0, 148.2, 155.1, 149.8, 160.0, 158.5, 145.0, 150.0, 152.3};
    int data_len = sizeof(market_data) / sizeof(market_data[0]);

    // 3. Read by the plugin via shared memory
    double volatility = calculate_variance(market_data, data_len);

    printf("[C Agent] 10-Day Market Volatility (Variance): %.4f\n", volatility);

    return 0;
}