//! VenomMemory - High-performance lock-free shared memory IPC
//!
//! This library provides ultra-low-latency inter-process communication
//! using shared memory with a Single Writer Multiple Readers (SWMR) pattern.
//!
//! # Architecture
//!
//! - **Single Writer (Daemon)**: Owns the shared memory, processes write commands
//! - **Multiple Readers (Shells)**: Read data directly, send write commands to daemon
//!
//! # Performance
//!
//! - Data reads: < 50ns (SeqLock)
//! - Command sends: < 100ns (MPSC lock-free queue)

pub mod error;
pub mod shm;
pub mod seqlock;
pub mod mpsc_queue;
pub mod channel;
pub mod bindings;

pub use error::{VenomError, Result};
pub use channel::{DaemonChannel, ShellChannel, ChannelConfig};
