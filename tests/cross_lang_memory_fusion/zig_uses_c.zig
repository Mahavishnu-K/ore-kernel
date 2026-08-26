const std = @import("std");
const ore = @import("ore_sys");

pub fn main() !void {
    std.debug.print("[Zig Host] Booting... I will use a C plugin to crunch my arrays.\n", .{});

    var plugin = try ore.Plugin.load("stats.wasi.so");

    // Bind the C function! (C's `double` is Zig's `f64`)
    const calculate_variance = try plugin.bind(fn ([*]const f64, i32) f64, "calculate_variance");

    const market_data = [_]f64{ 100.5, 105.2, 98.4, 110.1, 95.0 };
    
    // Call the C logic natively!
    const variance = calculate_variance(&market_data, @as(i32, @intCast(market_data.len)));

    std.debug.print("[Zig Host] C computed Variance: {d:.4}\n", .{variance});
}