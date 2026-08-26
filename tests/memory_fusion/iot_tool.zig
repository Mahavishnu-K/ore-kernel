const std = @import("std");
const ore = @import("ore_sys"); // Auto-injected by the CLI!

pub fn main() !void {
    std.debug.print("[Zig Agent] IoT Monitor Booting...\n", .{});

    // 1. Safe, elegant loading
    var plugin = try ore.Plugin.load("parser.wasi.so");

    // 2. Comptime Type-Safe Binding! No raw pointers, strings lengths, or typedefs!
    const parse = try plugin.bind(fn ([*]const u8, u32, u32) u32, "count_error_flags");

    // Simulated binary stream (0xFF is an error)
    const sensor_data = [_]u8{ 0x00, 0xFF, 0x01, 0x00, 0xFF, 0xFF, 0x02, 0x00 };
    
    // 3. Execution (Zero Copy!)
    const error_count = parse(&sensor_data, sensor_data.len, 0xFF);

    std.debug.print("[Zig Agent] Scan Complete. Found {} critical errors in stream.\n", .{error_count});
}