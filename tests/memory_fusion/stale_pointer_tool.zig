const std = @import("std");
const ore = @import("ore_sys"); // Auto-injected by the CLI!

// EXPECTED RESULT: this tool ends in a WebAssembly TRAP on purpose.
// After ore_dlclose, the kernel nulls the plugin's table slots, so calling a stale
// function pointer must trap instead of running freed code. The kernel keeps running.
pub fn main() !void {
    std.debug.print("[Zig Agent] Stale-pointer test booting...\n", .{});

    var plugin = try ore.Plugin.load("beacon.wasi.so");
    const ping = try plugin.bind(fn (u32) u32, "ping");

    const reply = ping(41);
    std.debug.print("[Zig Agent] ping(41) = {}\n", .{reply});
    if (reply != 42) {
        std.debug.print("[Zig Agent] FAIL ping returned the wrong value\n", .{});
        return;
    }
    std.debug.print("[Zig Agent] PASS plugin works before close\n", .{});

    // Keep a copy of the handle so we can try closing it a second time.
    var stale_copy = plugin;

    try plugin.close();
    std.debug.print("[Zig Agent] PASS close() succeeded\n", .{});

    const double_close_failed = blk: {
        stale_copy.close() catch |err| {
            std.debug.print("[Zig Agent] PASS double close failed with {s}\n", .{@errorName(err)});
            if (ore.lastError()) |msg| {
                std.debug.print("[Zig Agent] lastError = \"{s}\"\n", .{msg});
            }
            break :blk true;
        };
        break :blk false;
    };
    if (!double_close_failed) {
        std.debug.print("[Zig Agent] FAIL closing the same handle twice succeeded\n", .{});
    }

    std.debug.print("[Zig Agent] Calling the stale ping pointer. The sandbox should now TRAP...\n", .{});
    const after = ping(1);

    // Reaching this line means the table slot was NOT invalidated.
    std.debug.print("[Zig Agent] FAIL stale pointer still ran and returned {}\n", .{after});
}
