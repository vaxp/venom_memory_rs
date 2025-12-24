//! Example Shell (Reader/Client)
//!
//! This shell connects to a daemon's shared memory channel,
//! can read data and send commands.

use venom_memory::ShellChannel;
use std::io::{self, BufRead, Write};

fn print_help() {
    println!("Commands:");
    println!("  read          - Read current data from shared memory");
    println!("  send <cmd>    - Send a command to daemon");
    println!("  ping          - Shortcut for 'send ping'");
    println!("  time          - Shortcut for 'send time'");
    println!("  stats         - Shortcut for 'send stats'");
    println!("  bench <n>     - Run N ping-pong iterations");
    println!("  quit          - Ask daemon to quit");
    println!("  exit          - Exit this shell");
    println!("  help          - Show this help");
}

fn main() {
    let namespace = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "venom_demo".to_string());

    println!("╔══════════════════════════════════════════════════╗");
    println!("║        VenomMemory Shell (Rust Edition)          ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
    println!("[Shell] Connecting to namespace: {}", namespace);

    let shell = match ShellChannel::connect(&namespace) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Shell] Failed to connect: {}", e);
            eprintln!("[Shell] Make sure the daemon is running first!");
            std::process::exit(1);
        }
    };

    println!("[Shell] Connected! Client ID: {}", shell.client_id());
    println!("[Shell] Type 'help' for available commands");
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    // Check for single command mode
    if let Some(cmd) = std::env::args().nth(2) {
        let args: Vec<String> = std::env::args().skip(2).collect();
        let full_cmd = args.join(" ");
        execute_command(&shell, &full_cmd);
        return;
    }

    // Interactive mode
    loop {
        print!(">>> ");
        stdout.flush().unwrap();

        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        if input == "exit" {
            println!("[Shell] Goodbye!");
            break;
        }

        execute_command(&shell, input);
        println!();
    }
}

fn execute_command(shell: &ShellChannel, input: &str) {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];
    let arg = parts.get(1).unwrap_or(&"");

    match cmd {
        "help" => print_help(),
        
        "read" => {
            let mut buf = [0u8; 4096];
            let len = shell.read_data_with_len(&mut buf);
            if len > 0 {
                let data = String::from_utf8_lossy(&buf[..len.min(buf.len())]);
                println!("[Shell] Data ({} bytes): {}", len, data);
            } else {
                println!("[Shell] No data available");
            }
        }
        
        "send" => {
            if arg.is_empty() {
                println!("[Shell] Usage: send <command>");
                return;
            }
            send_and_receive(shell, arg);
        }
        
        "ping" | "time" | "pid" | "stats" | "quit" => {
            send_and_receive(shell, cmd);
        }
        
        "bench" => {
            let iterations: u64 = arg.parse().unwrap_or(1000);
            run_benchmark(shell, iterations);
        }
        
        _ => {
            // Treat unknown commands as direct send
            send_and_receive(shell, input);
        }
    }
}

fn send_and_receive(shell: &ShellChannel, cmd: &str) {
    println!("[Shell] Sending: {}", cmd);
    
    let start = std::time::Instant::now();
    
    // Send command
    shell.send_command(cmd.as_bytes());
    
    // Wait and read response (spin for a bit)
    let mut buf = [0u8; 4096];
    let mut attempts = 0;
    
    loop {
        let len = shell.read_data_with_len(&mut buf);
        if len > 0 {
            let elapsed = start.elapsed();
            let response = String::from_utf8_lossy(&buf[..len.min(buf.len())]);
            println!("[Shell] Response: {} ({:.2}µs)", response, elapsed.as_secs_f64() * 1_000_000.0);
            break;
        }
        
        attempts += 1;
        if attempts > 1_000_000 {
            println!("[Shell] Timeout waiting for response");
            break;
        }
        core::hint::spin_loop();
    }
}

fn run_benchmark(shell: &ShellChannel, iterations: u64) {
    println!("[Shell] Running {} iterations...", iterations);
    
    let cmd = b"ping";
    let mut buf = [0u8; 64];
    
    // Warmup
    for _ in 0..100 {
        shell.send_command(cmd);
        loop {
            if shell.read_data_with_len(&mut buf) > 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }
    
    // Benchmark
    let start = std::time::Instant::now();
    
    for _ in 0..iterations {
        shell.send_command(cmd);
        loop {
            if shell.read_data_with_len(&mut buf) > 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }
    
    let elapsed = start.elapsed();
    let total_ns = elapsed.as_nanos() as f64;
    let avg_ns = total_ns / iterations as f64;
    let throughput = iterations as f64 / elapsed.as_secs_f64();
    
    println!();
    println!("╔═══════════════════════════════════════╗");
    println!("║           Benchmark Results           ║");
    println!("╠═══════════════════════════════════════╣");
    println!("║ Iterations:  {:>20}   ║", iterations);
    println!("║ Total time:  {:>17.2} ms ║", elapsed.as_secs_f64() * 1000.0);
    println!("║ Avg latency: {:>17.2} ns ║", avg_ns);
    println!("║ Throughput:  {:>14.0} req/s ║", throughput);
    println!("╚═══════════════════════════════════════╝");
}
