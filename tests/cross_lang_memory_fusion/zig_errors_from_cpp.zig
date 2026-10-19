const std = @import("std");
const ore = @import("ore_sys");

// A Zig host exercises ore-ld's error reporting against the C++ `sanitizer.wasi.so` plugin:
// a failed load, a failed bind, then a successful call and a clean close.
pub fn main() !void {
    std.debug.print("[Zig Host] Booting... I will provoke ore-ld errors, then use a C++ plugin.\n", .{});
    var failures: u32 = 0;

    // 1. A failed load reports why.
    if (ore.Plugin.load("no_such_plugin.wasi.so")) |_| {
        std.debug.print("[Zig Host] FAIL loading a missing plugin succeeded\n", .{});
        failures += 1;
    } else |err| {
        std.debug.print("[Zig Host] PASS missing plugin failed with {s}\n", .{@errorName(err)});
        if (ore.lastError()) |msg| {
            std.debug.print("[Zig Host] lastError = \"{s}\"\n", .{msg});
        } else {
            std.debug.print("[Zig Host] FAIL lastError was empty after a failed load\n", .{});
            failures += 1;
        }
    }

    // The error is cleared once read.
    if (ore.lastError()) |msg| {
        std.debug.print("[Zig Host] FAIL lastError was not cleared: \"{s}\"\n", .{msg});
        failures += 1;
    } else {
        std.debug.print("[Zig Host] PASS lastError is cleared after it is read\n", .{});
    }

    var plugin = try ore.Plugin.load("sanitizer.wasi.so");

    // 2. A failed bind reports why.
    if (plugin.bind(fn () i32, "sanitize_everything")) |_| {
        std.debug.print("[Zig Host] FAIL binding a missing symbol succeeded\n", .{});
        failures += 1;
    } else |err| {
        std.debug.print("[Zig Host] PASS missing symbol failed with {s}\n", .{@errorName(err)});
        if (ore.lastError()) |msg| {
            std.debug.print("[Zig Host] lastError = \"{s}\"\n", .{msg});
        } else {
            std.debug.print("[Zig Host] FAIL lastError was empty after a failed bind\n", .{});
            failures += 1;
        }
    }

    // 3. The real symbol still works: C++ cleans Zig's buffer in place.
    const sanitize_spaces = try plugin.bind(fn ([*]u8, i32) i32, "sanitize_spaces");
    var buf = "Zig    hands    C++    its    buffer".*;
    const new_len = sanitize_spaces(&buf, @as(i32, @intCast(buf.len)));
    const cleaned = buf[0..@as(usize, @intCast(new_len))];
    std.debug.print("[Zig Host] C++ Cleaned: '{s}'\n", .{cleaned});
    if (std.mem.eql(u8, cleaned, "Zig hands C++ its buffer")) {
        std.debug.print("[Zig Host] PASS C++ plugin mutated Zig memory in place\n", .{});
    } else {
        std.debug.print("[Zig Host] FAIL unexpected sanitizer output\n", .{});
        failures += 1;
    }

    // 4. Clean close.
    try plugin.close();
    std.debug.print("[Zig Host] PASS close() succeeded\n", .{});

    if (failures == 0) {
        std.debug.print("[Zig Host] ALL CHECKS PASSED\n", .{});
    } else {
        std.debug.print("[Zig Host] {} CHECK(S) FAILED\n", .{failures});
        std.process.exit(1);
    }
}
