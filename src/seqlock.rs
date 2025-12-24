//! SeqLock - Sequence Lock for Single Writer Multiple Readers
//!
//! A SeqLock allows one writer and multiple readers to access data concurrently.
//! Readers never block - they simply retry if data changes during read.
//!
//! # Performance
//! - Write: ~10ns (two atomic increments + memcpy)
//! - Read: ~20-50ns (spin until consistent)

use std::sync::atomic::{AtomicU64, Ordering};

/// Cache line size for most modern x86_64 CPUs
const CACHE_LINE_SIZE: usize = 64;

/// Ensures the wrapped value is on its own cache line
#[repr(C, align(64))]
pub struct CacheAligned<T>(pub T);

/// SeqLock header stored in shared memory
#[repr(C)]
pub struct SeqLockHeader {
    /// Sequence number: odd = write in progress, even = stable
    sequence: CacheAligned<AtomicU64>,
    /// Size of the data region
    data_size: usize,
    /// Padding to ensure data starts on cache line boundary
    _pad: [u8; CACHE_LINE_SIZE - 16],
}

impl SeqLockHeader {
    /// Initialize a new SeqLock header
    ///
    /// # Safety
    /// The pointer must point to valid, properly aligned memory
    pub unsafe fn init(ptr: *mut Self, data_size: usize) {
        (*ptr).sequence.0 = AtomicU64::new(0);
        (*ptr).data_size = data_size;
    }

    /// Get the data size
    #[inline(always)]
    pub fn data_size(&self) -> usize {
        self.data_size
    }
}

/// Writer-side SeqLock operations
pub struct SeqLockWriter {
    header: *mut SeqLockHeader,
    data: *mut u8,
}

// SAFETY: SeqLockWriter only used by single writer
unsafe impl Send for SeqLockWriter {}

impl SeqLockWriter {
    /// Create a new writer from raw pointers
    ///
    /// # Safety
    /// - `header` must point to a valid, initialized SeqLockHeader
    /// - `data` must point to the data region immediately after the header
    /// - Only one SeqLockWriter should exist at a time
    pub unsafe fn from_raw(header: *mut SeqLockHeader, data: *mut u8) -> Self {
        Self { header, data }
    }

    /// Write data to the shared region
    ///
    /// This will:
    /// 1. Increment sequence to odd (signal write starting)
    /// 2. Copy data
    /// 3. Increment sequence to even (signal write complete)
    #[inline]
    pub fn write(&self, data: &[u8]) {
        let header = unsafe { &*self.header };
        let max_size = header.data_size;

        let len = data.len().min(max_size);

        // Increment to odd - write in progress
        header.sequence.0.fetch_add(1, Ordering::Release);

        // Write data
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.data, len);
        }

        // Memory fence to ensure all writes are visible
        std::sync::atomic::fence(Ordering::Release);

        // Increment to even - write complete
        header.sequence.0.fetch_add(1, Ordering::Release);
    }

    /// Write with length prefix (for variable-size data)
    #[inline]
    pub fn write_with_len(&self, data: &[u8]) {
        let header = unsafe { &*self.header };
        let max_size = header.data_size;

        let len = data.len().min(max_size - 8);

        // Increment to odd
        header.sequence.0.fetch_add(1, Ordering::Release);

        // Write length + data
        unsafe {
            let len_bytes = (len as u64).to_le_bytes();
            std::ptr::copy_nonoverlapping(len_bytes.as_ptr(), self.data, 8);
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.data.add(8), len);
        }

        std::sync::atomic::fence(Ordering::Release);

        // Increment to even
        header.sequence.0.fetch_add(1, Ordering::Release);
    }
}

/// Reader-side SeqLock operations
pub struct SeqLockReader {
    header: *const SeqLockHeader,
    data: *const u8,
}

// SAFETY: SeqLockReader is read-only and uses atomic operations
unsafe impl Send for SeqLockReader {}
unsafe impl Sync for SeqLockReader {}

impl SeqLockReader {
    /// Create a new reader from raw pointers
    ///
    /// # Safety
    /// - `header` must point to a valid SeqLockHeader
    /// - `data` must point to the data region
    pub unsafe fn from_raw(header: *const SeqLockHeader, data: *const u8) -> Self {
        Self { header, data }
    }

    /// Read data from the shared region
    ///
    /// This will spin until a consistent read is obtained.
    /// Returns the number of bytes read.
    #[inline]
    pub fn read(&self, buf: &mut [u8]) -> usize {
        let header = unsafe { &*self.header };
        let max_size = header.data_size.min(buf.len());

        loop {
            // Read sequence (must be even = no write in progress)
            let seq1 = header.sequence.0.load(Ordering::Acquire);
            if seq1 & 1 == 1 {
                // Write in progress, spin
                core::hint::spin_loop();
                continue;
            }

            // Read data
            unsafe {
                std::ptr::copy_nonoverlapping(self.data, buf.as_mut_ptr(), max_size);
            }

            // Memory fence
            std::sync::atomic::fence(Ordering::Acquire);

            // Check sequence again
            let seq2 = header.sequence.0.load(Ordering::Acquire);
            if seq1 == seq2 {
                // Consistent read!
                return max_size;
            }

            // Sequence changed, retry
            core::hint::spin_loop();
        }
    }

    /// Read data with length prefix
    ///
    /// Returns the actual data length (may be larger than buffer)
    #[inline]
    pub fn read_with_len(&self, buf: &mut [u8]) -> usize {
        let header = unsafe { &*self.header };

        loop {
            let seq1 = header.sequence.0.load(Ordering::Acquire);
            if seq1 & 1 == 1 {
                core::hint::spin_loop();
                continue;
            }

            // Read length
            let len = unsafe {
                let mut len_bytes = [0u8; 8];
                std::ptr::copy_nonoverlapping(self.data, len_bytes.as_mut_ptr(), 8);
                u64::from_le_bytes(len_bytes) as usize
            };

            let copy_len = len.min(buf.len());

            // Read data
            unsafe {
                std::ptr::copy_nonoverlapping(self.data.add(8), buf.as_mut_ptr(), copy_len);
            }

            std::sync::atomic::fence(Ordering::Acquire);

            let seq2 = header.sequence.0.load(Ordering::Acquire);
            if seq1 == seq2 {
                return len;
            }

            core::hint::spin_loop();
        }
    }

    /// Try to read once without spinning
    ///
    /// Returns `Some(bytes_read)` if successful, `None` if write in progress
    #[inline]
    pub fn try_read(&self, buf: &mut [u8]) -> Option<usize> {
        let header = unsafe { &*self.header };
        let max_size = header.data_size.min(buf.len());

        let seq1 = header.sequence.0.load(Ordering::Acquire);
        if seq1 & 1 == 1 {
            return None;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(self.data, buf.as_mut_ptr(), max_size);
        }

        std::sync::atomic::fence(Ordering::Acquire);

        let seq2 = header.sequence.0.load(Ordering::Acquire);
        if seq1 == seq2 {
            Some(max_size)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_seqlock_basic() {
        // Allocate aligned memory for header + data
        let layout = std::alloc::Layout::from_size_align(
            std::mem::size_of::<SeqLockHeader>() + 1024,
            64,
        )
        .unwrap();

        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        let header_ptr = ptr as *mut SeqLockHeader;
        let data_ptr = unsafe { ptr.add(std::mem::size_of::<SeqLockHeader>()) };

        unsafe {
            SeqLockHeader::init(header_ptr, 1024);
        }

        let writer = unsafe { SeqLockWriter::from_raw(header_ptr, data_ptr) };
        let reader = unsafe { SeqLockReader::from_raw(header_ptr, data_ptr) };

        // Write
        let test_data = b"Hello, SeqLock!";
        writer.write(test_data);

        // Read
        let mut buf = [0u8; 64];
        let len = reader.read(&mut buf);
        assert_eq!(&buf[..test_data.len()], test_data);

        unsafe {
            std::alloc::dealloc(ptr, layout);
        }
    }
}
