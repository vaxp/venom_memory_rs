//! Status Bar - Real VenomMemory Demo
//! 
//! This shell connects to the system_daemon and displays CPU/RAM usage
//! in a beautiful terminal status bar.

use std::thread;
use std::time::Duration;

use venom_memory::ShellChannel;

/// CPU stats - must match daemon's struct exactly!
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SystemStats {
    pub cpu_usage_percent: f32,
    pub cpu_cores: [f32; 16],
    pub core_count: u32,
    pub memory_used_mb: u32,
    pub memory_total_mb: u32,
    pub uptime_seconds: u64,
    pub timestamp_ns: u64,
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    
    if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

fn cpu_bar(percent: f32, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f32) as usize;
    let empty = width.saturating_sub(filled);
    
    let bar_char = if percent > 80.0 { "█" }
                   else if percent > 50.0 { "▓" }
                   else { "░" };
    
    format!("[{}{}]", bar_char.repeat(filled), " ".repeat(empty))
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         VenomMemory Status Bar (Shell)                        ║");
    println!("║         Reading system stats via shared memory                ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("⏳ Connecting to system_daemon...");
    
    // Connect to daemon
    let shell = match ShellChannel::connect("system_monitor") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ Failed to connect: {:?}", e);
            eprintln!("   Make sure system_daemon is running first!");
            eprintln!("   Run: cargo run --release --example system_daemon");
            return;
        }
    };
    
    println!("✅ Connected! Client ID: {}", shell.client_id());
    println!();
    
    // Clear screen
    print!("\x1B[2J\x1B[H");
    
    let mut buf = vec![0u8; std::mem::size_of::<SystemStats>() + 64];
    let mut frame = 0u64;
    
    loop {
        // Read from shared memory (lock-free, instant!)
        let len = shell.read_data(&mut buf);
        
        if len >= std::mem::size_of::<SystemStats>() {
            let stats: SystemStats = unsafe {
                std::ptr::read(buf.as_ptr() as *const SystemStats)
            };
            
            // Move cursor to top
            print!("\x1B[H");
            
            // Header
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║  🖥️  VenomMemory System Monitor                  Frame: {:>6} ║", frame);
            println!("╠═══════════════════════════════════════════════════════════════╣");
            
            // CPU Overall
            let cpu_color = if stats.cpu_usage_percent > 80.0 { "\x1B[31m" }
                           else if stats.cpu_usage_percent > 50.0 { "\x1B[33m" }
                           else { "\x1B[32m" };
            
            println!("║  CPU Total: {} {:>5.1}% {}                              ║",
                cpu_bar(stats.cpu_usage_percent, 20),
                stats.cpu_usage_percent,
                cpu_color
            );
            
            // Per-core (show up to 8)
            let cores_to_show = (stats.core_count as usize).min(8);
            for i in 0..cores_to_show {
                let usage = stats.cpu_cores[i];
                let color = if usage > 80.0 { "\x1B[31m" }
                           else if usage > 50.0 { "\x1B[33m" }
                           else { "\x1B[32m" };
                println!("║    Core {}: {} {:>5.1}% {}\x1B[0m                           ║",
                    i, cpu_bar(usage, 15), usage, color);
            }
            
            println!("╠═══════════════════════════════════════════════════════════════╣");
            
            // Memory
            let mem_percent = (stats.memory_used_mb as f32 / stats.memory_total_mb as f32) * 100.0;
            let mem_color = if mem_percent > 80.0 { "\x1B[31m" }
                           else if mem_percent > 50.0 { "\x1B[33m" }
                           else { "\x1B[32m" };
            
            println!("║  RAM: {} {:>5}/{:>5} MB ({}{:>5.1}%\x1B[0m)              ║",
                cpu_bar(mem_percent, 20),
                stats.memory_used_mb,
                stats.memory_total_mb,
                mem_color,
                mem_percent
            );
            
            println!("╠═══════════════════════════════════════════════════════════════╣");
            
            // Uptime
            println!("║  ⏱️  Uptime: {:>40}   ║", format_uptime(stats.uptime_seconds));
            
            println!("╚═══════════════════════════════════════════════════════════════╝");
            println!();
            println!("  Press Ctrl+C to exit");
            
            frame += 1;
        } else {
            println!("⏳ Waiting for data from daemon... (got {} bytes)", len);
        }
        
        // Update 10 times per second
        thread::sleep(Duration::from_millis(100));
    }
}
