//! Example Daemon (Writer/Server)
//!
//! This daemon creates a shared memory channel and processes commands
//! from connected shells.

use venom_memory::{ChannelConfig, DaemonChannel};
use std::io::{self, Write};

fn main() {
    let namespace = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "venom_demo".to_string());

    println!("╔══════════════════════════════════════════════════╗");
    println!("║       VenomMemory Daemon (Rust Edition)          ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
    println!("[Daemon] Starting with namespace: {}", namespace);

    let config = ChannelConfig {
        data_size: 64 * 1024,  // 64KB
        cmd_slots: 32,
        max_clients: 16,
    };

    let daemon = match DaemonChannel::create(&namespace, config) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[Daemon] Failed to create channel: {}", e);
            std::process::exit(1);
        }
    };

    println!("[Daemon] Channel created successfully");
    println!("[Daemon] Waiting for commands... (Ctrl+C to quit)");
    println!();

    let start_time = std::time::Instant::now();
    let mut cmd_count = 0u64;

    // Run the daemon loop
    daemon.run(|client_id, cmd| {
        cmd_count += 1;
        let cmd_str = String::from_utf8_lossy(cmd);
        
        println!("[Daemon] Client {} sent: {}", client_id, cmd_str.trim());

        // Process commands
        let response = match cmd_str.trim() {
            "ping" => "pong".to_string(),
            "time" => format!("Unix time: {}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()),
            "pid" => format!("Daemon PID: {}", std::process::id()),
            "stats" => {
                let elapsed = start_time.elapsed().as_secs_f64();
                format!("Commands: {}, Uptime: {:.1}s, Rate: {:.1}/s", 
                    cmd_count, elapsed, cmd_count as f64 / elapsed)
            },
            "help" => "Commands: ping, time, pid, stats, help, quit".to_string(),
            "quit" => {
                println!("[Daemon] Shutdown requested");
                "__SHUTDOWN__".to_string()
            },
            other => format!("Unknown command: {}", other),
        };

        println!("[Daemon] Response: {}", response);
        
        // Check for shutdown
        if response == "__SHUTDOWN__" {
            // Return empty to signal channel to check for shutdown
            return b"Goodbye!".to_vec();
        }

        response.into_bytes()
    });

    println!();
    println!("[Daemon] Shutting down...");
    println!("[Daemon] Total commands processed: {}", cmd_count);
}
