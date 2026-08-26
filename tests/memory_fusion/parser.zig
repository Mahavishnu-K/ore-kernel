export fn count_error_flags(data_ptr: [*]const u8, len: u32, target_flag: u32) u32 {
    var count: u32 = 0;
    var i: u32 = 0;
    
    while (i < len) : (i += 1) {
        // Cast the 32-bit flag back to u8 to compare against the byte array
        if (data_ptr[i] == @as(u8, @intCast(target_flag))) {
            count += 1;
        }
    }
    return count;
}