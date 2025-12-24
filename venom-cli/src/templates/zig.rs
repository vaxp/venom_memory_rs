//! Zig Templates for VenomMemory projects
//!
//! Generates a complete Zig project with:
//! - C ABI interop
//! - System monitor daemon
//! - Status bar client

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    crate::create_dir(&format!("{}/src", base));
    
    // Main source files
    crate::write_file(&format!("{}/src/venom.zig", base), &venom_zig(config));
    crate::write_file(&format!("{}/src/daemon.zig", base), &daemon_zig(config));
    crate::write_file(&format!("{}/src/client.zig", base), &client_zig(config));
    
    // Build file
    crate::write_file(&format!("{}/build.zig", base), &build_zig(config));
    
    // README
    crate::write_file(&format!("{}/README.md", base), &readme(config));
}

fn magic(channel: &str) -> u32 {
    channel.bytes().fold(0x564E4Fu32, |acc, b| acc.wrapping_add(b as u32))
}

fn pascal_case(s: &str) -> String {
    s.split(|c| c == '_' || c == '-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Venom bindings (Zig)
// ═══════════════════════════════════════════════════════════════════════════

fn venom_zig(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r##"//! VenomMemory Zig Bindings
const std = @import("std");

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

pub const channel_name = "{channel}";
pub const magic: u32 = 0x{magic:08X};
pub const data_size: usize = {data_size};
pub const cmd_slots: usize = {cmd_slots};
pub const max_clients: usize = {max_clients};
pub const max_cores: usize = 16;

// ═══════════════════════════════════════════════════════════════════════════
// State Structure (packed to match C layout)
// ═══════════════════════════════════════════════════════════════════════════

pub const State = extern struct {{
    magic_num: u32 = 0,
    version: u32 = 0,
    cpu_usage_percent: f32 = 0,
    cpu_cores: [max_cores]f32 = [_]f32{{0}} ** max_cores,
    core_count: u32 = 0,
    memory_used_mb: u32 = 0,
    memory_total_mb: u32 = 0,
    uptime_seconds: u64 = 0,
    update_counter: u64 = 0,
    timestamp_ns: u64 = 0,

    pub fn isValid(self: *const State) bool {{
        return self.magic_num == magic;
    }}

    pub fn memoryPercent(self: *const State) f32 {{
        if (self.memory_total_mb > 0) {{
            return @as(f32, @floatFromInt(self.memory_used_mb)) / @as(f32, @floatFromInt(self.memory_total_mb)) * 100.0;
        }}
        return 0;
    }}

    pub fn fromBytes(data: []const u8) State {{
        if (data.len < @sizeOf(State)) return State{{}};
        return std.mem.bytesToValue(State, data[0..@sizeOf(State)]);
    }}

    pub fn toBytes(self: *const State) [@sizeOf(State)]u8 {{
        return std.mem.toBytes(self.*);
    }}
}};

comptime {{
    if (@sizeOf(State) != 112) @compileError("State size mismatch");
}}

// ═══════════════════════════════════════════════════════════════════════════
// C FFI Bindings
// ═══════════════════════════════════════════════════════════════════════════

const VenomConfig = extern struct {{
    data_size: usize,
    cmd_slots: usize,
    max_clients: usize,
}};

extern fn venom_daemon_create(name: [*:0]const u8, config: VenomConfig) ?*anyopaque;
extern fn venom_daemon_destroy(handle: *anyopaque) void;
extern fn venom_daemon_write_data(handle: *anyopaque, data: [*]const u8, len: usize) void;

extern fn venom_shell_connect(name: [*:0]const u8) ?*anyopaque;
extern fn venom_shell_destroy(handle: *anyopaque) void;
extern fn venom_shell_read_data(handle: *anyopaque, buf: [*]u8, max_len: usize) usize;
extern fn venom_shell_id(handle: *anyopaque) u32;

// ═══════════════════════════════════════════════════════════════════════════
// Daemon Wrapper
// ═══════════════════════════════════════════════════════════════════════════

pub const Daemon = struct {{
    handle: *anyopaque,

    pub fn init() !Daemon {{
        const cfg = VenomConfig{{
            .data_size = data_size,
            .cmd_slots = cmd_slots,
            .max_clients = max_clients,
        }};
        const h = venom_daemon_create(channel_name, cfg) orelse return error.CreateFailed;
        return Daemon{{ .handle = h }};
    }}

    pub fn write(self: *Daemon, state: *const State) void {{
        const bytes = state.toBytes();
        venom_daemon_write_data(self.handle, &bytes, bytes.len);
    }}

    pub fn deinit(self: *Daemon) void {{
        venom_daemon_destroy(self.handle);
    }}
}};

// ═══════════════════════════════════════════════════════════════════════════
// Shell (Client) Wrapper
// ═══════════════════════════════════════════════════════════════════════════

pub const Shell = struct {{
    handle: *anyopaque,

    pub fn connect() !Shell {{
        const h = venom_shell_connect(channel_name) orelse return error.ConnectFailed;
        return Shell{{ .handle = h }};
    }}

    pub fn clientId(self: *Shell) u32 {{
        return venom_shell_id(self.handle);
    }}

    pub fn readState(self: *Shell) State {{
        var buf: [256]u8 = undefined;
        const n = venom_shell_read_data(self.handle, &buf, buf.len);
        return State.fromBytes(buf[0..n]);
    }}

    pub fn deinit(self: *Shell) void {{
        venom_shell_destroy(self.handle);
    }}
}};
"##,
        channel = config.channel,
        magic = magic(&config.channel),
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients,
    )
}

fn daemon_zig(config: &ProjectConfig) -> String {
    format!(r##"//! {name} System Monitor Daemon (Zig)
const std = @import("std");
const venom = @import("venom.zig");

var prev_total: [venom.max_cores + 1]u64 = [_]u64{{0}} ** (venom.max_cores + 1);
var prev_idle: [venom.max_cores + 1]u64 = [_]u64{{0}} ** (venom.max_cores + 1);

fn readCpu(state: *venom.State) void {{
    const file = std.fs.openFileAbsolute("/proc/stat", .{{}}) catch return;
    defer file.close();
    
    var buf: [4096]u8 = undefined;
    const reader = file.reader();
    
    var core_idx: usize = 0;
    while (reader.readUntilDelimiterOrEof(&buf, '\n') catch null) |line| {{
        if (core_idx > venom.max_cores) break;
        if (!std.mem.startsWith(u8, line, "cpu")) continue;
        
        var iter = std.mem.tokenizeAny(u8, line, " ");
        const cpu_name = iter.next() orelse continue;
        
        var values: [7]u64 = undefined;
        for (0..7) |i| {{
            values[i] = std.fmt.parseInt(u64, iter.next() orelse "0", 10) catch 0;
        }}
        
        const total = values[0] + values[1] + values[2] + values[3] + values[4] + values[5] + values[6];
        const idle_time = values[3] + values[4];
        const total_d = total -| prev_total[core_idx];
        const idle_d = idle_time -| prev_idle[core_idx];
        
        const usage: f32 = if (total_d > 0) (1.0 - @as(f32, @floatFromInt(idle_d)) / @as(f32, @floatFromInt(total_d))) * 100.0 else 0;
        
        if (std.mem.eql(u8, cpu_name, "cpu")) {{
            state.cpu_usage_percent = usage;
        }} else if (core_idx > 0 and core_idx <= venom.max_cores) {{
            state.cpu_cores[core_idx - 1] = usage;
        }}
        
        prev_total[core_idx] = total;
        prev_idle[core_idx] = idle_time;
        core_idx += 1;
    }}
    state.core_count = @intCast(if (core_idx > 1) core_idx - 1 else 0);
}}

fn readMemory(state: *venom.State) void {{
    const file = std.fs.openFileAbsolute("/proc/meminfo", .{{}}) catch return;
    defer file.close();
    
    var buf: [4096]u8 = undefined;
    const reader = file.reader();
    
    var total_kb: u64 = 0;
    var avail_kb: u64 = 0;
    
    while (reader.readUntilDelimiterOrEof(&buf, '\n') catch null) |line| {{
        if (std.mem.startsWith(u8, line, "MemTotal:")) {{
            var iter = std.mem.tokenizeAny(u8, line, " ");
            _ = iter.next();
            total_kb = std.fmt.parseInt(u64, iter.next() orelse "0", 10) catch 0;
        }} else if (std.mem.startsWith(u8, line, "MemAvailable:")) {{
            var iter = std.mem.tokenizeAny(u8, line, " ");
            _ = iter.next();
            avail_kb = std.fmt.parseInt(u64, iter.next() orelse "0", 10) catch 0;
        }}
    }}
    state.memory_total_mb = @intCast(total_kb / 1024);
    state.memory_used_mb = @intCast((total_kb - avail_kb) / 1024);
}}

fn readUptime(state: *venom.State) void {{
    const file = std.fs.openFileAbsolute("/proc/uptime", .{{}}) catch return;
    defer file.close();
    
    var buf: [64]u8 = undefined;
    const n = file.read(&buf) catch return;
    
    var iter = std.mem.tokenizeAny(u8, buf[0..n], " ");
    const uptime_str = iter.next() orelse return;
    const dot_idx = std.mem.indexOf(u8, uptime_str, ".") orelse uptime_str.len;
    state.uptime_seconds = std.fmt.parseInt(u64, uptime_str[0..dot_idx], 10) catch 0;
}}

pub fn main() !void {{
    const stdout = std.io.getStdOut().writer();
    
    try stdout.print("🖥️  {name} System Monitor (Zig)\n", .{{}});
    try stdout.print("═══════════════════════════════════════════════════════════════\n", .{{}});
    
    var daemon = venom.Daemon.init() catch {{
        try stdout.print("❌ Failed to create daemon\n", .{{}});
        return;
    }};
    defer daemon.deinit();
    
    try stdout.print("✅ Channel: {{s}}\n", .{{venom.channel_name}});
    try stdout.print("🚀 Publishing... (Ctrl+C to stop)\n\n", .{{}});
    
    var state = venom.State{{
        .magic_num = venom.magic,
        .version = 1,
    }};
    
    while (true) {{
        readCpu(&state);
        readMemory(&state);
        readUptime(&state);
        state.update_counter += 1;
        state.timestamp_ns = @intCast(std.time.nanoTimestamp());
        
        daemon.write(&state);
        
        try stdout.print("\r🖥️  CPU: {{d:.1}}% | RAM: {{d}}/{{d}} MB | #{{d}}   ", .{{
            state.cpu_usage_percent,
            state.memory_used_mb,
            state.memory_total_mb,
            state.update_counter,
        }});
        
        std.time.sleep(100 * std.time.ns_per_ms);
    }}
}}
"##, name = config.name)
}

fn client_zig(config: &ProjectConfig) -> String {
    format!(r##"//! {name} Status Bar Client (Zig) - with Benchmarking
const std = @import("std");
const venom = @import("venom.zig");

const green = "\x1b[92m";
const yellow = "\x1b[93m";
const red = "\x1b[91m";
const cyan = "\x1b[96m";
const reset = "\x1b[0m";

// Latency tracking
var g_latency_min: f64 = std.math.floatMax(f64);
var g_latency_max: f64 = 0.0;
var g_latency_sum: f64 = 0.0;
var g_latency_count: u64 = 0;

fn printBar(writer: anytype, pct: f32, width: usize) !void {{
    const filled = @as(usize, @intFromFloat((pct / 100.0) * @as(f32, @floatFromInt(width))));
    const color = if (pct > 80) red else if (pct > 50) yellow else green;
    
    try writer.print("[", .{{}});
    for (0..width) |i| {{
        if (i < filled) {{
            try writer.print("{{s}}█{{s}}", .{{ color, reset }});
        }} else {{
            try writer.print(" ", .{{}});
        }}
    }}
    try writer.print("]", .{{}});
}}

pub fn main() !void {{
    const stdout = std.io.getStdOut().writer();
    
    try stdout.print("╔═══════════════════════════════════════════════════════════════╗\n", .{{}});
    try stdout.print("║   🖥️  {name} Status Bar (Zig)                                  ║\n", .{{}});
    try stdout.print("╚═══════════════════════════════════════════════════════════════╝\n\n", .{{}});
    
    var shell = venom.Shell.connect() catch {{
        try stdout.print("❌ Failed to connect - is daemon running?\n", .{{}});
        return;
    }};
    defer shell.deinit();
    
    try stdout.print("✅ Connected! ID: {{d}}\n", .{{shell.clientId()}});
    try stdout.print("📊 Reading stats... (Ctrl+C to exit)\n\n", .{{}});
    std.time.sleep(1 * std.time.ns_per_s);
    
    var frame: u64 = 0;
    while (true) {{
        // ═══════════════════════════════════════════════════════════════════
        // 📊 BENCHMARK: Measure read latency
        // ═══════════════════════════════════════════════════════════════════
        const t_start = std.time.nanoTimestamp();
        const state = shell.readState();
        const t_end = std.time.nanoTimestamp();
        const latency_us = @as(f64, @floatFromInt(t_end - t_start)) / 1000.0;
        
        // Update stats
        if (latency_us < g_latency_min) g_latency_min = latency_us;
        if (latency_us > g_latency_max) g_latency_max = latency_us;
        g_latency_sum += latency_us;
        g_latency_count += 1;
        const avg_us = g_latency_sum / @as(f64, @floatFromInt(g_latency_count));
        
        if (state.isValid()) {{
            try stdout.print("\x1b[2J\x1b[H", .{{}});
            try stdout.print("╔═══════════════════════════════════════════════════════════════╗\n", .{{}});
            try stdout.print("║  🖥️  {name} Monitor (Zig)         Frame: {{d:<6}}              ║\n", .{{frame}});
            try stdout.print("╠═══════════════════════════════════════════════════════════════╣\n", .{{}});
            try stdout.print("║  CPU: ", .{{}});
            try printBar(stdout, state.cpu_usage_percent, 25);
            try stdout.print(" {{d:5.1}}%             ║\n", .{{state.cpu_usage_percent}});
            try stdout.print("╠═══════════════════════════════════════════════════════════════╣\n", .{{}});
            
            for (0..state.core_count) |i| {{
                try stdout.print("║  Core {{d}}: ", .{{i}});
                try printBar(stdout, state.cpu_cores[i], 20);
                try stdout.print(" {{d:5.1}}%                ║\n", .{{state.cpu_cores[i]}});
            }}
            
            try stdout.print("╠═══════════════════════════════════════════════════════════════╣\n", .{{}});
            try stdout.print("║  RAM: ", .{{}});
            try printBar(stdout, state.memoryPercent(), 25);
            try stdout.print(" {{d}}/{{d}} MB      ║\n", .{{state.memory_used_mb, state.memory_total_mb}});
            try stdout.print("╠═══════════════════════════════════════════════════════════════╣\n", .{{}});
            try stdout.print("║  ⏱️ Uptime: {{d}}h {{d}}m                                        ║\n", .{{
                state.uptime_seconds / 3600,
                (state.uptime_seconds % 3600) / 60,
            }});
            try stdout.print("╠═══════════════════════════════════════════════════════════════╣\n", .{{}});
            try stdout.print("║  📊 {{s}}Read Latency:{{s}} {{d:.2}} µs (min: {{d:.2}}, max: {{d:.2}}, avg: {{d:.2}})  ║\n", 
                .{{ cyan, reset, latency_us, g_latency_min, g_latency_max, avg_us }});
            try stdout.print("╚═══════════════════════════════════════════════════════════════╝\n", .{{}});
            try stdout.print("  Cores: {{d}} | Updates: {{d}} | Ctrl+C to exit\n", .{{state.core_count, state.update_counter}});
            frame += 1;
        }}
        std.time.sleep(100 * std.time.ns_per_ms);
    }}
}}
"##, name = config.name)
}

fn build_zig(config: &ProjectConfig) -> String {
    format!(r##"const std = @import("std");

pub fn build(b: *std.Build) void {{
    const target = b.standardTargetOptions(.{{}});
    const optimize = b.standardOptimizeOption(.{{}});

    // Daemon
    const daemon = b.addExecutable(.{{
        .name = "{name}_daemon",
        .root_source_file = .{{ .path = "src/daemon.zig" }},
        .target = target,
        .optimize = optimize,
    }});
    daemon.addLibraryPath(.{{ .path = "lib" }});
    daemon.linkSystemLibrary("venom_memory");
    daemon.linkLibC();
    daemon.addRPath(.{{ .path = "lib" }});
    b.installArtifact(daemon);

    // Client
    const client = b.addExecutable(.{{
        .name = "{name}_client",
        .root_source_file = .{{ .path = "src/client.zig" }},
        .target = target,
        .optimize = optimize,
    }});
    client.addLibraryPath(.{{ .path = "lib" }});
    client.linkSystemLibrary("venom_memory");
    client.linkLibC();
    client.addRPath(.{{ .path = "lib" }});
    b.installArtifact(client);

    // Run steps
    const run_daemon = b.addRunArtifact(daemon);
    const run_client = b.addRunArtifact(client);
    b.step("run-daemon", "Run the daemon").dependOn(&run_daemon.step);
    b.step("run-client", "Run the client").dependOn(&run_client.step);
}}
"##, name = config.name)
}

fn readme(config: &ProjectConfig) -> String {
    format!(r#"# {name} (Zig)

VenomMemory Zig system monitor with native C interop.

## Quick Start

```bash
# Build
zig build

# Terminal 1 - Daemon
zig build run-daemon

# Terminal 2 - Client
zig build run-client
```

## Configuration

| Setting | Value |
|---------|-------|
| Channel | `{channel}` |
| Magic | `0x{magic:08X}` |
"#,
        name = config.name,
        channel = config.channel,
        magic = magic(&config.channel)
    )
}
