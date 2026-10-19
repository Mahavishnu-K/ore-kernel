#![no_std]
use ore_sys::ore_export;

// Pricing model v2: the "updated risk formula", a 3x markup.
// Exports the same symbols as pricing_v1.rs, so a host can swap one for the other.
ore_export! {
    fn quote(base: u32) -> u32 {
        base * 3
    }
}

ore_export! {
    fn model_version() -> u32 {
        2
    }
}
