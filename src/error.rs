//! Error types for VenomMemory

use std::io;
use thiserror::Error;

/// Result type for VenomMemory operations
pub type Result<T> = std::result::Result<T, VenomError>;

/// Errors that can occur in VenomMemory operations
#[derive(Debug, Error)]
pub enum VenomError {
    /// Failed to create shared memory
    #[error("Failed to create shared memory '{name}': {source}")]
    ShmCreate {
        name: String,
        #[source]
        source: io::Error,
    },

    /// Failed to open shared memory
    #[error("Failed to open shared memory '{name}': {source}")]
    ShmOpen {
        name: String,
        #[source]
        source: io::Error,
    },

    /// Failed to map memory
    #[error("Failed to map memory: {0}")]
    Mmap(#[source] io::Error),

    /// Failed to truncate shared memory
    #[error("Failed to set shared memory size: {0}")]
    Truncate(#[source] io::Error),

    /// Invalid channel magic number
    #[error("Invalid channel magic number: expected 0x{expected:08X}, got 0x{got:08X}")]
    InvalidMagic { expected: u32, got: u32 },

    /// Buffer overflow
    #[error("Buffer overflow: max {max} bytes, got {got} bytes")]
    BufferOverflow { max: usize, got: usize },

    /// Command queue is full
    #[error("Command queue is full")]
    QueueFull,

    /// Command queue is empty
    #[error("Command queue is empty")]
    QueueEmpty,

    /// Invalid client ID
    #[error("Invalid client ID: {0}")]
    InvalidClientId(u32),

    /// Namespace too long
    #[error("Namespace too long: max {max} chars, got {got}")]
    NamespaceTooLong { max: usize, got: usize },
}
