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

pub const OreError = error{
    PluginLoadFailed,
    SymbolNotFound,
};

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
};