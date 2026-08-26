#include <ore.h>

// ORE_PLUGIN safely abstracts the visibility attributes
ORE_PLUGIN double calculate_variance(const double* prices, int length) {
    if (length == 0) return 0.0;
    
    // 1. Calculate Mean
    double sum = 0.0;
    for(int i = 0; i < length; i++) {
        sum += prices[i];
    }
    double mean = sum / length;
    
    // 2. Calculate Variance
    double variance = 0.0;
    for(int i = 0; i < length; i++) {
        double diff = prices[i] - mean;
        variance += diff * diff;
    }
    return variance / length;
}