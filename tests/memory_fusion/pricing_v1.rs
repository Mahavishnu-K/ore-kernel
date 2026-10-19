#![no_std]
use ore_sys::ore_export;

// Pricing model v1: a flat 2x markup.
// Used with pricing_v2.rs to test hot-swapping a plugin under a live host.
ore_export! {
    fn quote(base: u32) -> u32 {
        base * 2
    }
}

ore_export! {
    fn model_version() -> u32 {
        1
    }
}
