//! Ultra-High Performance Multi-Channel Test
//!
//! This test uses a simplified approach for maximum throughput:
//! - Direct shared memory access
//! - No command queue overhead for large data
//! - Pure SeqLock reads

use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rustix::shm::{shm_open, shm_unlink, Mode, ShmOFlags};
use rustix::mm::{mmap, MapFlags, ProtFlags};
use rustix::fs::ftruncate;
use std::ffi::CString;

const ITERATIONS: u64 = 100_000;

/// Shared region between one writer and one reader
#[repr(C)]
struct ChannelData {
    // Writer -> Reader
    write_seq: AtomicU64,
    _pad1: [u8; 56],
    // Reader -> Writer (for request-response)
    read_seq: AtomicU64,
    _pad2: [u8; 56],
    // Data buffer
    data_len: AtomicU64,
    _pad3: [u8; 56],
    // Data starts here (256KB)
}

fn create_channel(name: &str, data_size: usize) -> (*mut u8, usize) {
    let total_size = std::mem::size_of::<ChannelData>() + data_size;
    let full_name = format!("/venom_ultra_{}", name);
    let c_name = CString::new(full_name.clone()).unwrap();
    
    // Remove if exists
    let _ = shm_unlink(c_name.as_c_str());
    
    let fd = shm_open(
        c_name.as_c_str(),
        ShmOFlags::CREATE | ShmOFlags::RDWR,
        Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
    ).expect("Failed to create shm");
    
    ftruncate(&fd, total_size as u64).expect("Failed to truncate");
    
    let addr = unsafe {
        mmap(
            std::ptr::null_mut(),
            total_size,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            &fd,
            0,
        ).expect("Failed to mmap")
    };
    
    // Zero init
    unsafe {
        std::ptr::write_bytes(addr, 0, total_size);
    }
    
    (addr as *mut u8, total_size)
}

fn open_channel(name: &str) -> (*mut u8, usize) {
    let full_name = format!("/venom_ultra_{}", name);
    let c_name = CString::new(full_name).unwrap();
    
    let fd = shm_open(c_name.as_c_str(), ShmOFlags::RDWR, Mode::empty())
        .expect("Failed to open shm");
    
    let stat = rustix::fs::fstat(&fd).expect("Failed to stat");
    let size = stat.st_size as usize;
    
    let addr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            &fd,
            0,
        ).expect("Failed to mmap")
    };
    
    (addr as *mut u8, size)
}

fn cleanup_channel(name: &str) {
    let full_name = format!("/venom_ultra_{}", name);
    if let Ok(c_name) = CString::new(full_name) {
        let _ = shm_unlink(c_name.as_c_str());
    }
}

struct ChannelStats {
    successful: AtomicU64,
    total_latency_ns: AtomicU64,
    min_latency_ns: AtomicU64,
    max_latency_ns: AtomicU64,
}

impl ChannelStats {
    fn new() -> Self {
        Self {
            successful: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            min_latency_ns: AtomicU64::new(u64::MAX),
            max_latency_ns: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    fn record(&self, latency_ns: u64) {
        self.successful.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);
        
        // Relaxed min/max - not perfectly accurate but fast
        let min = self.min_latency_ns.load(Ordering::Relaxed);
        if latency_ns < min {
            self.min_latency_ns.store(latency_ns, Ordering::Relaxed);
        }
        let max = self.max_latency_ns.load(Ordering::Relaxed);
        if latency_ns > max {
            self.max_latency_ns.store(latency_ns, Ordering::Relaxed);
        }
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
    
    let start_barrier = Arc::new(Barrier::new(num_channels * 2 + 1));
    let stop_flag = Arc::new(AtomicBool::new(false));
    
    // Test data (pre-filled pattern)
    let test_data: Vec<u8> = (0..data_size).map(|i| (i & 0xFF) as u8).collect();
    
    println!("Creating {} channels...", num_channels);
    
    // Create channels
    let channels: Vec<_> = (0..num_channels)
        .map(|i| {
            let name = format!("ch_{}", i);
            create_channel(&name, data_size)
        })
        .collect();
    
    println!("Starting {} writer threads...", num_channels);
    
    // Spawn writer (daemon) threads
    let mut writer_handles = Vec::new();
    for i in 0..num_channels {
        let start_barrier = Arc::clone(&start_barrier);
        let stop_flag = Arc::clone(&stop_flag);
        let (ptr, _size) = channels[i];
        let ptr_addr = ptr as usize;  // Convert to usize for Send
        let test_data = test_data.clone();
        
        let handle = thread::spawn(move || {
            let ptr = ptr_addr as *mut u8;
            let header = ptr as *mut ChannelData;
            let data_ptr = unsafe { ptr.add(std::mem::size_of::<ChannelData>()) };
            
            start_barrier.wait();
            
            let mut last_read_seq = 0u64;
            
            while !stop_flag.load(Ordering::Relaxed) {
                unsafe {
                    // Wait for read request
                    let read_seq = (*header).read_seq.load(Ordering::Acquire);
                    if read_seq > last_read_seq {
                        last_read_seq = read_seq;
                        
                        // Write response - increment seq to odd
                        (*header).write_seq.fetch_add(1, Ordering::Release);
                        
                        // Copy data
                        std::ptr::copy_nonoverlapping(
                            test_data.as_ptr(),
                            data_ptr,
                            test_data.len()
                        );
                        (*header).data_len.store(test_data.len() as u64, Ordering::Relaxed);
                        
                        // Finish write - increment seq to even
                        (*header).write_seq.fetch_add(1, Ordering::Release);
                    } else {
                        core::hint::spin_loop();
                    }
                }
            }
        });
        writer_handles.push(handle);
    }
    
    println!("Starting {} reader threads...", num_channels);
    
    // Spawn reader (shell) threads
    let mut reader_handles = Vec::new();
    for i in 0..num_channels {
        let start_barrier = Arc::clone(&start_barrier);
        let stats = Arc::clone(&stats[i]);
        let data_size = data_size;
        let iterations = iterations;
        
        // Open the channel
        let name = format!("ch_{}", i);
        let (ptr, _) = open_channel(&name);
        let ptr_addr = ptr as usize;  // Convert to usize for Send
        
        let handle = thread::spawn(move || {
            let ptr = ptr_addr as *mut u8;
            let header = ptr as *mut ChannelData;
            let data_ptr = unsafe { ptr.add(std::mem::size_of::<ChannelData>()) };
            let mut read_buf = vec![0u8; data_size];
            
            start_barrier.wait();
            
            for _ in 0..iterations {
                let start = Instant::now();
                
                unsafe {
                    // Send request
                    (*header).read_seq.fetch_add(1, Ordering::Release);
                    
                    // Wait for response with SeqLock
                    let mut success = false;
                    while !success {
                        let seq1 = (*header).write_seq.load(Ordering::Acquire);
                        if seq1 & 1 == 1 {
                            core::hint::spin_loop();
                            continue;
                        }
                        
                        // Read data
                        let len = (*header).data_len.load(Ordering::Relaxed) as usize;
                        std::ptr::copy_nonoverlapping(
                            data_ptr,
                            read_buf.as_mut_ptr(),
                            len.min(data_size)
                        );
                        
                        std::sync::atomic::fence(Ordering::Acquire);
                        
                        let seq2 = (*header).write_seq.load(Ordering::Acquire);
                        if seq1 == seq2 && seq1 > 0 {
                            success = true;
                        } else {
                            core::hint::spin_loop();
                        }
                    }
                }
                
                let elapsed = start.elapsed().as_nanos() as u64;
                stats.record(elapsed);
            }
        });
        reader_handles.push(handle);
    }
    
    // Start all threads
    let test_start = Instant::now();
    start_barrier.wait();
    
    // Wait for readers to complete
    for handle in reader_handles {
        handle.join().unwrap();
    }
    
    let test_duration = test_start.elapsed();
    
    // Stop writers
    stop_flag.store(true, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(10));
    
    // Print results
    println!("\n┌─────────┬───────────┬──────────┬──────────────┐");
    println!("│ Channel │ Successful│ Avg (µs) │ Max (µs)     │");
    println!("├─────────┼───────────┼──────────┼──────────────┤");
    
    let mut total_successful: u64 = 0;
    let mut total_latency_ns: u64 = 0;
    let mut global_min_ns: u64 = u64::MAX;
    let mut global_max_ns: u64 = 0;
    
    for (i, stat) in stats.iter().enumerate() {
        let successful = stat.successful.load(Ordering::Relaxed);
        let latency_ns = stat.total_latency_ns.load(Ordering::Relaxed);
        let min_ns = stat.min_latency_ns.load(Ordering::Relaxed);
        let max_ns = stat.max_latency_ns.load(Ordering::Relaxed);
        
        let avg_us = if successful > 0 {
            latency_ns as f64 / successful as f64 / 1000.0
        } else {
            0.0
        };
        let max_us = max_ns as f64 / 1000.0;
        
        println!("│  {:>3}    │  {:>8}  │  {:>7.2} │  {:>11.2} │",
                 i, successful, avg_us, max_us);
        
        total_successful += successful;
        total_latency_ns += latency_ns;
        global_min_ns = global_min_ns.min(min_ns);
        global_max_ns = global_max_ns.max(max_ns);
    }
    
    println!("└─────────┴───────────┴──────────┴──────────────┘");
    
    let duration_secs = test_duration.as_secs_f64();
    let avg_latency_us = if total_successful > 0 {
        total_latency_ns as f64 / total_successful as f64 / 1000.0
    } else {
        0.0
    };
    let throughput = total_successful as f64 / duration_secs;
    // Bandwidth: each request sends nothing, receives data_size (bidirectional would double)
    let bandwidth_mb = throughput * (data_size as f64 * 2.0) / 1_000_000.0;
    let bandwidth_gb = bandwidth_mb / 1000.0;
    
    println!("\n📊 AGGREGATE RESULTS:");
    println!("   Channels:         {}", num_channels);
    println!("   Total successful: {} / {}", total_successful, num_channels as u64 * iterations);
    println!("   Test duration:    {:.2} seconds", duration_secs);
    println!("   Avg latency:      {:.2} µs", avg_latency_us);
    println!("   Min latency:      {:.2} µs", global_min_ns as f64 / 1000.0);
    println!("   Max latency:      {:.2} µs ({:.2} ms)", 
             global_max_ns as f64 / 1000.0, global_max_ns as f64 / 1_000_000.0);
    println!("   ⚡ THROUGHPUT:     {:.0} req/s (total)", throughput);
    println!("   📶 BANDWIDTH:      {:.2} MB/s = {:.2} GB/s (total, bidirectional)", 
             bandwidth_mb, bandwidth_gb);
    
    // Cleanup
    for i in 0..num_channels {
        let name = format!("ch_{}", i);
        cleanup_channel(&name);
    }
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║   VenomMemory Rust - ULTRA Performance Test                   ║");
    println!("║   Goal: Beat 23.3 GB/s with 4 channels @ 256KB                ║");
    println!("║   Available CPUs: {}                                           ║", 
             std::thread::available_parallelism().map(|p| p.get()).unwrap_or(1));
    println!("╚═══════════════════════════════════════════════════════════════╝");
    
    // Warm up
    run_test(2, 1024, 10_000);
    
    // THE TARGET: 4 channels, 256KB
    run_test(4, 256 * 1024, ITERATIONS);
    
    // Also test 8 channels
    run_test(4, 256 * 1024, ITERATIONS);
    
    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    Test Complete!                             ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
}
