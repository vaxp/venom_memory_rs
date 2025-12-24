//! System Monitor Daemon - Real VenomMemory Demo
//! 
//! This daemon reads CPU usage from /proc/stat and writes to shared memory.
//! Any number of shells can read this data instantly.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::{Duration, Instant};

use venom_memory::{DaemonChannel, ChannelConfig};

/// CPU stats from /proc/stat
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SystemStats {
    pub cpu_usage_percent: f32,      // Overall CPU usage
    pub cpu_cores: [f32; 16],        // Per-core usage (up to 16 cores)
    pub core_count: u32,             // Actual number of cores
    pub memory_used_mb: u32,         // Used RAM in MB
    pub memory_total_mb: u32,        // Total RAM in MB
    pub uptime_seconds: u64,         // System uptime
    pub timestamp_ns: u64,           // When this was measured
}

#[derive(Default)]
struct CpuTimes {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
}

impl CpuTimes {
    fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq
    }
    
    fn active(&self) -> u64 {
        self.user + self.nice + self.system + self.irq + self.softirq
    }
}

fn read_cpu_times() -> Vec<CpuTimes> {
    let file = File::open("/proc/stat").expect("Cannot open /proc/stat");
    let reader = BufReader::new(file);
    let mut result = Vec::new();
    
    for line in reader.lines() {
        let line = line.unwrap();
        if line.starts_with("cpu") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 {
                result.push(CpuTimes {
                    user: parts[1].parse().unwrap_or(0),
                    nice: parts[2].parse().unwrap_or(0),
                    system: parts[3].parse().unwrap_or(0),
                    idle: parts[4].parse().unwrap_or(0),
                    iowait: parts[5].parse().unwrap_or(0),
                    irq: parts[6].parse().unwrap_or(0),
                    softirq: parts[7].parse().unwrap_or(0),
                });
            }
        }
    }
    result
}

fn read_memory_info() -> (u32, u32) {
    let file = File::open("/proc/meminfo").expect("Cannot open /proc/meminfo");
    let reader = BufReader::new(file);
    let mut total = 0u64;
    let mut available = 0u64;
    
    for line in reader.lines() {
        let line = line.unwrap();
        if line.starts_with("MemTotal:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            total = parts[1].parse().unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            available = parts[1].parse().unwrap_or(0);
        }
    }
    
    let total_mb = (total / 1024) as u32;
    let used_mb = ((total - available) / 1024) as u32;
    (used_mb, total_mb)
}

fn read_uptime() -> u64 {
    let content = std::fs::read_to_string("/proc/uptime").unwrap_or_default();
    let parts: Vec<&str> = content.split_whitespace().collect();
    if !parts.is_empty() {
        parts[0].parse::<f64>().unwrap_or(0.0) as u64
    } else {
        0
    }
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         VenomMemory System Monitor Daemon                     ║");
    println!("║         Reading CPU/Memory and sharing via VenomMemory        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    
    // Create channel with enough space for SystemStats
    let config = ChannelConfig {
        data_size: std::mem::size_of::<SystemStats>() + 64,
        cmd_slots: 16,
        max_clients: 8,
    };
    
    let daemon = DaemonChannel::create("system_monitor", config)
        .expect("Failed to create daemon channel");
    
    println!("\n✅ Daemon started on channel: system_monitor");
    println!("📊 Stats struct size: {} bytes", std::mem::size_of::<SystemStats>());
    println!("🔄 Refreshing every 100ms...\n");
    
    let mut prev_times = read_cpu_times();
    let start = Instant::now();
    let mut fake_until: Option<Instant> = None;
    
    loop {
        // Check for commands from shells
        let mut cmd_buf = [0u8; 64];
        if let Some((client_id, len)) = daemon.try_recv_command(&mut cmd_buf) {
            let cmd_str = String::from_utf8_lossy(&cmd_buf[..len]);
            println!("\n📥 Received command from client {}: {}", client_id, cmd_str);
            
            if cmd_str == "FAKE_100" {
                fake_until = Some(Instant::now() + Duration::from_secs(5));
                println!("⚡ Faking 100% CPU for 5 seconds!");
            }
        }
        
        // Read current CPU times
        let curr_times = read_cpu_times();
        
        // Calculate usage
        let mut stats = SystemStats::default();
        stats.core_count = (curr_times.len().saturating_sub(1)).min(16) as u32;
        
        // Check if we should fake 100%
        let fake_mode = fake_until.map(|t| Instant::now() < t).unwrap_or(false);
        
        if fake_mode {
            // Fake 100% CPU
            stats.cpu_usage_percent = 100.0;
            for i in 0..stats.core_count as usize {
                stats.cpu_cores[i] = 100.0;
            }
        } else {
            // Real CPU measurement
            fake_until = None;
            
            // Overall CPU (index 0 is aggregate)
            if !prev_times.is_empty() && !curr_times.is_empty() {
                let prev = &prev_times[0];
                let curr = &curr_times[0];
                
                let total_diff = curr.total().saturating_sub(prev.total());
                let active_diff = curr.active().saturating_sub(prev.active());
                
                if total_diff > 0 {
                    stats.cpu_usage_percent = (active_diff as f32 / total_diff as f32) * 100.0;
                }
            }
            
            // Per-core CPU
            for i in 1..curr_times.len().min(17) {
                if i < prev_times.len() {
                    let prev = &prev_times[i];
                    let curr = &curr_times[i];
                    
                    let total_diff = curr.total().saturating_sub(prev.total());
                    let active_diff = curr.active().saturating_sub(prev.active());
                    
                    if total_diff > 0 {
                        stats.cpu_cores[i - 1] = (active_diff as f32 / total_diff as f32) * 100.0;
                    }
                }
            }
        }
        
        // Memory
        let (used, total) = read_memory_info();
        stats.memory_used_mb = used;
        stats.memory_total_mb = total;
        
        // Uptime & timestamp
        stats.uptime_seconds = read_uptime();
        stats.timestamp_ns = start.elapsed().as_nanos() as u64;
        
        // Write to shared memory!
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &stats as *const SystemStats as *const u8,
                std::mem::size_of::<SystemStats>()
            )
        };
        daemon.write_data(bytes);
        
        // Debug output
        let mode_str = if fake_mode { "🔴 FAKE" } else { "🟢 REAL" };
        print!("\r{} CPU: {:5.1}% | RAM: {}/{} MB | Uptime: {}s    ",
            mode_str,
            stats.cpu_usage_percent,
            stats.memory_used_mb,
            stats.memory_total_mb,
            stats.uptime_seconds
        );
        use std::io::Write;
        std::io::stdout().flush().ok();
        
        prev_times = curr_times;
        thread::sleep(Duration::from_millis(100));
    }
}
