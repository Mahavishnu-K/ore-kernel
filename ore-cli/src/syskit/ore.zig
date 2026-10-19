// RAW WEBASSEMBLY IMPORTS
const ore_dlopen = @extern(
    *const fn ([*]const u8, u32) callconv(.c) i32,
    .{
        .name = "ore_dlopen",
        .linkage = .strong,
    },
);
const ore_dlsym = @extern(
    *const fn (i32, [*]const u8, u32) callconv(.c) i32,
    .{
        .name = "ore_dlsym",
        .linkage = .strong,
    },
);

const ore_dlclose = @extern(
    *const fn (i32) callconv(.c) i32,
    .{
        .name = "ore_dlclose",
        .linkage = .strong,
    },
);

const ore_dlerror = @extern(
    *const fn ([*]u8, u32) callconv(.c) i32,
    .{
        .name = "ore_dlerror",
        .linkage = .strong,
    },
);

pub const OreError = error{
    PluginLoadFailed,
    SymbolNotFound,
    PluginCloseFailed,
};

// ERROR INSPECTION (Zero-Allocation)
var last_error_buf: [512]u8 = undefined;

/// Fetches the last human/machine-readable error string emitted by the Ore Kernel, and clears it.
/// Returns null if there was no error.
/// The returned slice points into a shared static buffer: it is only valid until the next call.
pub fn lastError() ?[]const u8 {
    const bytes = ore_dlerror(&last_error_buf, last_error_buf.len);
    if (bytes <= 0) return null;
    return last_error_buf[0..@as(usize, @intCast(bytes))];
}

// THE HOST API
pub const Plugin = struct {
    handle: i32,

    pub fn load(plugin_name: []const u8) OreError!Plugin {
        const handle = ore_dlopen(plugin_name.ptr, @as(u32, @intCast(plugin_name.len)));
        
        if (handle <= 0) {
            return error.PluginLoadFailed;
        }
        
        return Plugin{ .handle = handle };
    }

    /// Dynamically extracts the function and casts it to the exact type
    pub fn bind(self: Plugin, comptime FuncSig: type, symbol: []const u8) OreError!*const FuncSig {
        const func_idx = ore_dlsym(self.handle, symbol.ptr, @as(u32, @intCast(symbol.len)));
        
        if (func_idx <= 0) {
            return error.SymbolNotFound;
        }

        // Cast it to the requested signature.
        return @as(*const FuncSig, @ptrFromInt(@as(usize, @intCast(func_idx))));
    }

    /// Deterministically closes the plugin, poisoning its table slots in the Kernel
    pub fn close(self: *Plugin) OreError!void {
        if (self.handle > 0) {
            const res = ore_dlclose(self.handle);
            if (res != 0) return error.PluginCloseFailed;
            self.handle = 0;
        }
    }
};
