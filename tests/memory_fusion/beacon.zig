// A minimal plugin used to verify that ore_dlclose invalidates function pointers.
export fn ping(seq: u32) u32 {
    return seq + 1;
}
