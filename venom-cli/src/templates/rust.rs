//! Rust Templates for VenomMemory projects
//! 
//! Generates a complete Rust project with:
//! - Cargo.toml configured for local venom_memory build
//! - src/lib.rs with shared protocol types
//! - src/bin/daemon.rs - System monitor daemon
//! - src/bin/client.rs - Status display client
//! - build.rs for custom library linking

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    crate::create_dir(&format!("{}/src/bin", base));
    
    crate::write_file(&format!("{}/Cargo.toml", base), &cargo_toml(config));
    crate::write_file(&format!("{}/build.rs", base), &build_rs(config));
    crate::write_file(&format!("{}/.cargo/config.toml", base), &cargo_config(config));
    crate::write_file(&format!("{}/src/lib.rs", base), &lib_rs(config));
    crate::write_file(&format!("{}/src/bin/daemon.rs", base), &daemon_rs(config));
    crate::write_file(&format!("{}/src/bin/client.rs", base), &client_rs(config));
    crate::write_file(&format!("{}/README.md", base), &readme(config));
}

fn magic(channel: &str) -> u32 {
    channel.bytes().fold(0x564E4Fu32, |acc, b| acc.wrapping_add(b as u32))
}

// Cargo.toml - uses local venom_memory via build.rs linking
fn cargo_toml(config: &ProjectConfig) -> String {
    format!(r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
build = "build.rs"

# Uses bundled library via FFI + ctrlc for signal handling
[dependencies]
ctrlc = "3.4"

[[bin]]
name = "daemon"
path = "src/bin/daemon.rs"

[[bin]]
name = "client"
path = "src/bin/client.rs"
"#, name = config.name)
}

// build.rs - tells cargo where to find the library
fn build_rs(_config: &ProjectConfig) -> String {
    r#"fn main() {
    // Tell cargo to look for libvenom_memory.so in the lib/ directory
    println!("cargo:rustc-link-search=native={}", 
        std::env::current_dir().unwrap().join("lib").display());
    println!("cargo:rustc-link-lib=dylib=venom_memory");
    
    // Set rpath so the binary can find the library at runtime
    // Binary is in target/debug/ or target/release/, lib is in lib/
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../../lib");
}
"#.to_string()
}

// .cargo/config.toml - runtime library path
fn cargo_config(_config: &ProjectConfig) -> String {
    r#"[env]
LD_LIBRARY_PATH = { value = "lib", relative = true }
"#.to_string()
}

fn lib_rs(config: &ProjectConfig) -> String {
    format!(r#"//! {name} Protocol - Shared types for daemon/client communication
//!
//! This module defines:
//! - Channel configuration constants
//! - State struct (daemon publishes, clients read)
//! - Command struct (clients send, daemon receives)

pub const CHANNEL_NAME: &str = "{channel}";
pub const MAGIC: u32 = 0x{magic:08X};
pub const DATA_SIZE: usize = {data_size};
pub const CMD_SLOTS: usize = {cmd_slots};
pub const MAX_CLIENTS: usize = {max_clients};
pub const MAX_CORES: usize = 16;

/// System state published by daemon
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct State {{
    pub magic: u32,
    pub version: u32,
    pub cpu_usage_percent: f32,
    pub cpu_cores: [f32; MAX_CORES],
    pub core_count: u32,
    pub memory_used_mb: u32,
    pub memory_total_mb: u32,
    pub uptime_seconds: u64,
    pub update_counter: u64,
    pub timestamp_ns: u64,
}}

/// Command types
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum CmdType {{
    Refresh = 1,
    SetInterval = 2,
}}

/// Command sent from client to daemon
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Command {{
    pub cmd: u8,
    pub _pad: [u8; 3],
    pub value: i32,
}}

// ═══════════════════════════════════════════════════════════════════════════
// FFI Bindings to VenomMemory (lib/libvenom_memory.so)
// ═══════════════════════════════════════════════════════════════════════════

#[repr(C)]
pub struct VenomConfig {{
    pub data_size: usize,
    pub cmd_slots: usize,
    pub max_clients: usize,
}}

#[link(name = "venom_memory")]
extern "C" {{
    pub fn venom_daemon_create(name: *const i8, config: VenomConfig) -> *mut std::ffi::c_void;
    pub fn venom_daemon_destroy(handle: *mut std::ffi::c_void);
    pub fn venom_daemon_write_data(handle: *mut std::ffi::c_void, data: *const u8, len: usize);
    pub fn venom_daemon_try_recv_command(handle: *mut std::ffi::c_void, buf: *mut u8, max_len: usize, out_client_id: *mut u32) -> usize;
    
    pub fn venom_shell_connect(name: *const i8) -> *mut std::ffi::c_void;
    pub fn venom_shell_destroy(handle: *mut std::ffi::c_void);
    pub fn venom_shell_read_data(handle: *mut std::ffi::c_void, buf: *mut u8, max_len: usize) -> usize;
    pub fn venom_shell_id(handle: *mut std::ffi::c_void) -> u32;
}}

/// Safe wrapper for VenomMemory Daemon
pub struct Daemon {{
    handle: *mut std::ffi::c_void,
}}

impl Daemon {{
    pub fn create(name: &str) -> Option<Self> {{
        let c_name = std::ffi::CString::new(name).ok()?;
        let config = VenomConfig {{
            data_size: DATA_SIZE,
            cmd_slots: CMD_SLOTS,
            max_clients: MAX_CLIENTS,
        }};
        let handle = unsafe {{ venom_daemon_create(c_name.as_ptr(), config) }};
        if handle.is_null() {{ None }} else {{ Some(Self {{ handle }}) }}
    }}
    
    pub fn write_data(&self, data: &[u8]) {{
        unsafe {{ venom_daemon_write_data(self.handle, data.as_ptr(), data.len()) }};
    }}
    
    pub fn try_recv_command(&self, buf: &mut [u8]) -> Option<(u32, usize)> {{
        let mut client_id = 0u32;
        let len = unsafe {{ venom_daemon_try_recv_command(self.handle, buf.as_mut_ptr(), buf.len(), &mut client_id) }};
        if len > 0 {{ Some((client_id, len)) }} else {{ None }}
    }}
}}

impl Drop for Daemon {{
    fn drop(&mut self) {{
        unsafe {{ venom_daemon_destroy(self.handle) }};
    }}
}}

/// Safe wrapper for VenomMemory Shell (client)
pub struct Shell {{
    handle: *mut std::ffi::c_void,
}}

impl Shell {{
    pub fn connect(name: &str) -> Option<Self> {{
        let c_name = std::ffi::CString::new(name).ok()?;
        let handle = unsafe {{ venom_shell_connect(c_name.as_ptr()) }};
        if handle.is_null() {{ None }} else {{ Some(Self {{ handle }}) }}
    }}
    
    pub fn client_id(&self) -> u32 {{
        unsafe {{ venom_shell_id(self.handle) }}
    }}
    
    pub fn read_data(&self, buf: &mut [u8]) -> usize {{
        unsafe {{ venom_shell_read_data(self.handle, buf.as_mut_ptr(), buf.len()) }}
    }}
}}

impl Drop for Shell {{
    fn drop(&mut self) {{
        unsafe {{ venom_shell_destroy(self.handle) }};
    }}
}}
"#,
        name = config.name,
        channel = config.channel,
        magic = magic(&config.channel),
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients
    )
}

fn daemon_rs(config: &ProjectConfig) -> String {
    let name_snake = config.name.replace("-", "_");
    
    format!(r##"//! {name} System Monitor Daemon
//!
//! Reads CPU/RAM/Uptime from /proc and publishes via VenomMemory IPC.

use {name_snake}::{{CHANNEL_NAME, MAGIC, MAX_CORES, State, Daemon}};
use std::fs::File;
use std::io::{{BufRead, BufReader}};
use std::time::{{Duration, Instant}};

fn main() {{
    println!("🖥️  {name} System Monitor (VenomMemory)");
    println!("═══════════════════════════════════════════════════════════════");
    
    let daemon = Daemon::create(CHANNEL_NAME).expect("Failed to create channel");
    println!("✅ Channel: {{}} | Publishing...", CHANNEL_NAME);
    
    let mut state = State::default();
    state.magic = MAGIC;
    state.version = 1;
    
    let start = Instant::now();
    let mut prev_total = vec![0u64; MAX_CORES + 1];
    let mut prev_idle = vec![0u64; MAX_CORES + 1];
    let mut cmd_buf = [0u8; 64];
    
    loop {{
        // Read CPU from /proc/stat
        if let Ok(f) = File::open("/proc/stat") {{
            let mut core_idx = 0;
            for line in BufReader::new(f).lines().flatten() {{
                if !line.starts_with("cpu") {{ continue; }}
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 8 {{
                    let total: u64 = parts[1..8].iter().filter_map(|s| s.parse::<u64>().ok()).sum();
                    let idle: u64 = parts[4].parse().unwrap_or(0) + parts[5].parse().unwrap_or(0);
                    let total_d = total - prev_total[core_idx];
                    let idle_d = idle - prev_idle[core_idx];
                    let usage = if total_d > 0 {{ (1.0 - idle_d as f32 / total_d as f32) * 100.0 }} else {{ 0.0 }};
                    if parts[0] == "cpu" {{ state.cpu_usage_percent = usage; }}
                    else if core_idx > 0 && core_idx <= MAX_CORES {{ state.cpu_cores[core_idx - 1] = usage; }}
                    prev_total[core_idx] = total;
                    prev_idle[core_idx] = idle;
                    core_idx += 1;
                }}
            }}
            state.core_count = if core_idx > 1 {{ (core_idx - 1) as u32 }} else {{ 0 }};
        }}
        
        // Read Memory from /proc/meminfo
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {{
            let mut total = 0u64;
            let mut avail = 0u64;
            for line in content.lines() {{
                if line.starts_with("MemTotal:") {{ total = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0); }}
                if line.starts_with("MemAvailable:") {{ avail = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0); }}
            }}
            state.memory_total_mb = (total / 1024) as u32;
            state.memory_used_mb = ((total - avail) / 1024) as u32;
        }}
        
        // Read Uptime from /proc/uptime
        if let Ok(content) = std::fs::read_to_string("/proc/uptime") {{
            state.uptime_seconds = content.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u64;
        }}
        
        // Publish state
        state.update_counter += 1;
        state.timestamp_ns = start.elapsed().as_nanos() as u64;
        let bytes = unsafe {{ std::slice::from_raw_parts(&state as *const State as *const u8, std::mem::size_of::<State>()) }};
        daemon.write_data(bytes);
        
        // Check for commands
        if let Some((client_id, _)) = daemon.try_recv_command(&mut cmd_buf) {{
            println!("\n📥 Command from client {{}}", client_id);
        }}
        
        print!("\r🖥️  CPU: {{:5.1}}% | RAM: {{}}/{{}} MB | #{{}}   ", 
            state.cpu_usage_percent, state.memory_used_mb, state.memory_total_mb, state.update_counter);
        std::thread::sleep(Duration::from_millis(100));
    }}
}}
"##,
        name = config.name,
        name_snake = name_snake
    )
}

fn client_rs(config: &ProjectConfig) -> String {
    let name_snake = config.name.replace("-", "_");
    
    format!(r##"//! {name} Status Bar Client - with Benchmarking
//!
//! Connects to daemon and displays live system stats.
//! Includes read latency measurements.

use {name_snake}::{{CHANNEL_NAME, MAGIC, State, Shell}};
use std::time::Instant;

// ANSI colors
const CYAN: &str = "\x1b[96m";
const RST: &str = "\x1b[0m";

fn main() {{
    println!("🖥️  {name} Status Bar (Rust)");
    println!("═══════════════════════════════════════════════════════════════");
    
    let shell = Shell::connect(CHANNEL_NAME).expect("Failed to connect - is daemon running?");
    println!("✅ Connected! ID: {{}}", shell.client_id());
    
    let mut buf = vec![0u8; std::mem::size_of::<State>() + 64];
    
    // Latency tracking
    let mut latency_min = f64::MAX;
    let mut latency_max = 0.0_f64;
    let mut latency_sum = 0.0_f64;
    let mut latency_count = 0_u64;
    let mut frame = 0_u64;
    
    // Register Ctrl+C handler
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {{
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    }}).ok();
    
    while running.load(std::sync::atomic::Ordering::SeqCst) {{
        // ═══════════════════════════════════════════════════════════════════
        // 📊 BENCHMARK: Measure read latency
        // ═══════════════════════════════════════════════════════════════════
        let t_start = Instant::now();
        let len = shell.read_data(&mut buf);
        let latency_us = t_start.elapsed().as_nanos() as f64 / 1000.0;
        
        // Update stats
        if latency_us < latency_min {{ latency_min = latency_us; }}
        if latency_us > latency_max {{ latency_max = latency_us; }}
        latency_sum += latency_us;
        latency_count += 1;
        let avg_us = latency_sum / latency_count as f64;
        
        if len >= std::mem::size_of::<State>() {{
            let state: State = unsafe {{ std::ptr::read(buf.as_ptr() as *const State) }};
            if state.magic == MAGIC {{
                print!("\x1b[2J\x1b[H");
                println!("╔═══════════════════════════════════════════════════════════════╗");
                println!("║  🖥️  {name} Monitor (Rust)      Frame: {{:<6}}                  ║", frame);
                println!("╠═══════════════════════════════════════════════════════════════╣");
                println!("║  CPU: {{:5.1}}%  |  RAM: {{}}/{{}} MB  |  Uptime: {{}}h{{}}m             ║",
                    state.cpu_usage_percent, state.memory_used_mb, state.memory_total_mb,
                    state.uptime_seconds / 3600, (state.uptime_seconds % 3600) / 60);
                println!("╠═══════════════════════════════════════════════════════════════╣");
                println!("║  📊 {{}}Read Latency:{{}} {{:.2}} µs (min: {{:.2}}, max: {{:.2}}, avg: {{:.2}})  ║",
                    CYAN, RST, latency_us, latency_min, latency_max, avg_us);
                println!("╚═══════════════════════════════════════════════════════════════╝");
                println!("  Cores: {{}} | Updates: {{}}", state.core_count, state.update_counter);
                frame += 1;
            }}
        }}
        std::thread::sleep(std::time::Duration::from_millis(100));
    }}
    
    // Print final stats
    println!("\n\n📊 {{}}Final Latency Stats (Rust):{{}}", CYAN, RST);
    println!("   Samples: {{}}", latency_count);
    println!("   Min: {{:.2}} µs", latency_min);
    println!("   Max: {{:.2}} µs", latency_max);
    println!("   Avg: {{:.2}} µs", latency_sum / latency_count as f64);
    println!("\n👋 Goodbye!");
}}
"##,
        name = config.name,
        name_snake = name_snake
    )
}

fn readme(config: &ProjectConfig) -> String {
    format!(r#"# {name} (Rust)

VenomMemory system monitor - reads CPU/RAM/Uptime and displays live stats.

## Quick Start

```bash
# Terminal 1 - Start daemon
cargo run --bin daemon

# Terminal 2 - Start client  
cargo run --bin client
```

## Configuration

| Setting | Value |
|---------|-------|
| Channel | `{channel}` |
| Data Size | {data_size} bytes |
| Command Slots | {cmd_slots} |
| Max Clients | {max_clients} |

## Project Structure

- `src/lib.rs` - Protocol types and FFI bindings
- `src/bin/daemon.rs` - System monitor daemon
- `src/bin/client.rs` - Status display client
- `lib/libvenom_memory.so` - VenomMemory library (bundled)
"#,
        name = config.name,
        channel = config.channel,
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients
    )
}
