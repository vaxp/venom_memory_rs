//! Multi-Core Stress Test - Matching the C benchmark
//!
//! This test runs multiple parallel channels to measure maximum throughput.

use venom_memory::{ChannelConfig, DaemonChannel, ShellChannel};
use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const ITERATIONS_PER_CHANNEL: u64 = 100_000; // Reduced for faster testing

struct ChannelStats {
    successful: AtomicU64,
    errors: AtomicU64,
    total_latency_ns: AtomicU64,
    min_latency_ns: AtomicU64,
    max_latency_ns: AtomicU64,
}

impl ChannelStats {
    fn new() -> Self {
        Self {
            successful: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            min_latency_ns: AtomicU64::new(u64::MAX),
            max_latency_ns: AtomicU64::new(0),
        }
    }

    fn record(&self, latency_ns: u64) {
        self.successful.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);
        
        // Update min
        let mut min = self.min_latency_ns.load(Ordering::Relaxed);
        while latency_ns < min {
            match self.min_latency_ns.compare_exchange_weak(min, latency_ns, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(m) => min = m,
            }
        }
        
        // Update max
        let mut max = self.max_latency_ns.load(Ordering::Relaxed);
        while latency_ns > max {
            match self.max_latency_ns.compare_exchange_weak(max, latency_ns, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(m) => max = m,
            }
        }
    }

    fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

fn run_test(num_channels: usize, data_size: usize, iterations: u64) {
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing: {} parallel channels, {} bytes ({:.2} KB)", 
             num_channels, data_size, data_size as f64 / 1024.0);
    println!("═══════════════════════════════════════════════════════════════");
    
    let stats: Vec<Arc<ChannelStats>> = (0..num_channels)
        .map(|_| Arc::new(ChannelStats::new()))
        .collect();
    
    let daemons_ready = Arc::new(Barrier::new(num_channels + 1));
    let start_barrier = Arc::new(Barrier::new(num_channels * 2 + 1));
    let stop_flag = Arc::new(AtomicBool::new(false));
    
    // Test data
    let test_data: Vec<u8> = (0..data_size).map(|i| (i & 0xFF) as u8).collect();
    
    println!("Creating {} daemon channels...", num_channels);
    
    // Spawn daemon threads
    let mut daemon_handles = Vec::new();
    for i in 0..num_channels {
        let daemons_ready = Arc::clone(&daemons_ready);
        let start_barrier = Arc::clone(&start_barrier);
        let stop_flag = Arc::clone(&stop_flag);
        let data_size = data_size;
        
        let handle = thread::spawn(move || {
            let namespace = format!("bench_ch_{}", i);
            let config = ChannelConfig {
                data_size: data_size + 1024,
                cmd_slots: 64,
                max_clients: 4,
            };
            
            let daemon = DaemonChannel::create(&namespace, config).unwrap();
            
            // Signal ready
            daemons_ready.wait();
            start_barrier.wait();
            
            // Process commands
            let mut cmd_buf = vec![0u8; data_size + 64];
            while !stop_flag.load(Ordering::Relaxed) {
                if let Some((client_id, cmd_len)) = daemon.try_recv_command(&mut cmd_buf) {
                    // Echo back
                    daemon.write_data_with_len(&cmd_buf[..cmd_len]);
                } else {
                    core::hint::spin_loop();
                }
            }
        });
        daemon_handles.push(handle);
    }
    
    // Wait for daemons to be ready
    daemons_ready.wait();
    thread::sleep(Duration::from_millis(10));
    
    println!("Starting {} shell clients...", num_channels);
    
    // Spawn shell threads
    let mut shell_handles = Vec::new();
    for i in 0..num_channels {
        let start_barrier = Arc::clone(&start_barrier);
        let stats = Arc::clone(&stats[i]);
        let test_data = test_data.clone();
        let iterations = iterations;
        
        let handle = thread::spawn(move || {
            let namespace = format!("bench_ch_{}", i);
            let shell = ShellChannel::connect(&namespace).unwrap();
            
            let mut response_buf = vec![0u8; test_data.len() + 64];
            
            // Wait for all threads
            start_barrier.wait();
            
            for _ in 0..iterations {
                let start = Instant::now();
                
                // Send command
                shell.send_command(&test_data);
                
                // Wait for response
                loop {
                    let len = shell.read_data_with_len(&mut response_buf);
                    if len > 0 {
                        break;
                    }
                    core::hint::spin_loop();
                }
                
                let elapsed = start.elapsed().as_nanos() as u64;
                stats.record(elapsed);
            }
        });
        shell_handles.push(handle);
    }
    
    // Start all threads
    let test_start = Instant::now();
    start_barrier.wait();
    
    // Wait for shells to complete
    for handle in shell_handles {
        handle.join().unwrap();
    }
    
    let test_duration = test_start.elapsed();
    
    // Stop daemons
    stop_flag.store(true, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(10));
    
    // Print results
    println!("\n┌─────────┬───────────┬────────┬──────────┬──────────────┐");
    println!("│ Channel │ Successful│ Errors │ Avg (µs) │ Max (µs)     │");
    println!("├─────────┼───────────┼────────┼──────────┼──────────────┤");
    
    let mut total_successful: u64 = 0;
    let mut total_errors: u64 = 0;
    let mut total_latency_ns: u64 = 0;
    let mut global_min_ns: u64 = u64::MAX;
    let mut global_max_ns: u64 = 0;
    
    for (i, stat) in stats.iter().enumerate() {
        let successful = stat.successful.load(Ordering::Relaxed);
        let errors = stat.errors.load(Ordering::Relaxed);
        let latency_ns = stat.total_latency_ns.load(Ordering::Relaxed);
        let min_ns = stat.min_latency_ns.load(Ordering::Relaxed);
        let max_ns = stat.max_latency_ns.load(Ordering::Relaxed);
        
        let avg_us = if successful > 0 {
            latency_ns as f64 / successful as f64 / 1000.0
        } else {
            0.0
        };
        let max_us = max_ns as f64 / 1000.0;
        
        println!("│  {:>3}    │  {:>8}  │  {:>4}  │  {:>7.2} │  {:>11.2} │",
                 i, successful, errors, avg_us, max_us);
        
        total_successful += successful;
        total_errors += errors;
        total_latency_ns += latency_ns;
        global_min_ns = global_min_ns.min(min_ns);
        global_max_ns = global_max_ns.max(max_ns);
    }
    
    println!("└─────────┴───────────┴────────┴──────────┴──────────────┘");
    
    let duration_secs = test_duration.as_secs_f64();
    let avg_latency_us = if total_successful > 0 {
        total_latency_ns as f64 / total_successful as f64 / 1000.0
    } else {
        0.0
    };
    let throughput = total_successful as f64 / duration_secs;
    // Bandwidth: each request sends data_size and receives data_size (bidirectional)
    let bandwidth_mb = throughput * (data_size as f64 * 2.0) / 1_000_000.0;
    let bandwidth_gb = bandwidth_mb / 1000.0;
    
    println!("\n📊 AGGREGATE RESULTS:");
    println!("   Channels:         {}", num_channels);
    println!("   Total successful: {} / {}", total_successful, num_channels as u64 * iterations);
    println!("   Total errors:     {}", total_errors);
    println!("   Test duration:    {:.2} seconds", duration_secs);
    println!("   Avg latency:      {:.2} µs", avg_latency_us);
    println!("   Min latency:      {:.2} µs", global_min_ns as f64 / 1000.0);
    println!("   Max latency:      {:.2} µs ({:.2} ms)", 
             global_max_ns as f64 / 1000.0, global_max_ns as f64 / 1_000_000.0);
    println!("   ⚡ THROUGHPUT:     {:.0} req/s (total)", throughput);
    println!("   📶 BANDWIDTH:      {:.2} MB/s = {:.2} GB/s (total, bidirectional)", 
             bandwidth_mb, bandwidth_gb);
    
    // Wait for daemon threads (they might be stuck, so we'll just detach)
    // In a real implementation we'd send a shutdown command
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         VenomMemory Rust - Multi-Core Stress Test             ║");
    println!("║         Available CPUs: {}                                     ║", 
             std::thread::available_parallelism().map(|p| p.get()).unwrap_or(1));
    println!("╚═══════════════════════════════════════════════════════════════╝");
    
    // Test configurations matching the C benchmark
    let configs = [
        // (channels, data_size, iterations)
        (4, 1024, ITERATIONS_PER_CHANNEL),           // 1 KB - warm up
        (4, 256 * 1024, ITERATIONS_PER_CHANNEL),     // 256 KB - THE TARGET
    ];
    
    for (channels, data_size, iterations) in configs {
        run_test(channels, data_size, iterations);
    }
    
    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    Test Complete!                             ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
}
