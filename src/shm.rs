//! Low-level POSIX shared memory operations

use crate::error::{Result, VenomError};
use rustix::fd::OwnedFd;
use rustix::fs::ftruncate;
use rustix::mm::{mmap, munmap, MapFlags, ProtFlags};
use rustix::shm::{shm_open, shm_unlink, Mode, ShmOFlags};
use std::ffi::CString;
use std::ptr::NonNull;

const VENOM_SHM_PREFIX: &str = "/venom_";
const MAX_NAME_LEN: usize = 255 - VENOM_SHM_PREFIX.len();

/// Handle to a shared memory region
pub struct VenomShm {
    #[allow(dead_code)]
    fd: OwnedFd,
    addr: NonNull<u8>,
    size: usize,
    name: String,
    is_owner: bool,
}

// SAFETY: VenomShm can be safely shared between threads
// The shared memory region itself is synchronized via atomic operations
unsafe impl Send for VenomShm {}
unsafe impl Sync for VenomShm {}

impl VenomShm {
    /// Create a new shared memory region
    ///
    /// # Arguments
    /// * `name` - Unique name for the shared memory (will be prefixed with "/venom_")
    /// * `size` - Size in bytes
    ///
    /// # Returns
    /// A new VenomShm handle on success
    pub fn create(name: &str, size: usize) -> Result<Self> {
        if name.len() > MAX_NAME_LEN {
            return Err(VenomError::NamespaceTooLong {
                max: MAX_NAME_LEN,
                got: name.len(),
            });
        }

        let full_name = format!("{}{}", VENOM_SHM_PREFIX, name);
        let c_name = CString::new(full_name.clone()).unwrap();

        // Try to create exclusively first, fall back to open if exists
        let fd = match shm_open(
            c_name.as_c_str(),
            ShmOFlags::CREATE | ShmOFlags::EXCL | ShmOFlags::RDWR,
            Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH,
        ) {
            Ok(fd) => fd,
            Err(_) => {
                // Already exists, try to open
                shm_open(c_name.as_c_str(), ShmOFlags::RDWR, Mode::empty()).map_err(|e| {
                    VenomError::ShmCreate {
                        name: name.to_string(),
                        source: e.into(),
                    }
                })?
            }
        };

        // Set size
        ftruncate(&fd, size as u64).map_err(|e| VenomError::Truncate(e.into()))?;

        // Map to memory
        let addr = unsafe {
            mmap(
                std::ptr::null_mut(),
                size,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                &fd,
                0,
            )
            .map_err(|e| VenomError::Mmap(e.into()))?
        };

        let addr = NonNull::new(addr.cast::<u8>()).expect("mmap returned null");

        // Zero initialize
        unsafe {
            std::ptr::write_bytes(addr.as_ptr(), 0, size);
        }

        Ok(Self {
            fd,
            addr,
            size,
            name: name.to_string(),
            is_owner: true,
        })
    }

    /// Open an existing shared memory region
    pub fn open(name: &str) -> Result<Self> {
        let full_name = format!("{}{}", VENOM_SHM_PREFIX, name);
        let c_name = CString::new(full_name).unwrap();

        let fd = shm_open(c_name.as_c_str(), ShmOFlags::RDWR, Mode::empty()).map_err(|e| {
            VenomError::ShmOpen {
                name: name.to_string(),
                source: e.into(),
            }
        })?;

        // Get size from file
        let stat = rustix::fs::fstat(&fd).map_err(|e| VenomError::ShmOpen {
            name: name.to_string(),
            source: e.into(),
        })?;
        let size = stat.st_size as usize;

        // Map to memory
        let addr = unsafe {
            mmap(
                std::ptr::null_mut(),
                size,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                &fd,
                0,
            )
            .map_err(|e| VenomError::Mmap(e.into()))?
        };

        let addr = NonNull::new(addr.cast::<u8>()).expect("mmap returned null");

        Ok(Self {
            fd,
            addr,
            size,
            name: name.to_string(),
            is_owner: false,
        })
    }

    /// Get raw pointer to shared memory
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut u8 {
        self.addr.as_ptr()
    }

    /// Get size of shared memory region
    #[inline(always)]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the name of shared memory
    #[inline(always)]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if this handle owns the shared memory
    #[inline(always)]
    pub fn is_owner(&self) -> bool {
        self.is_owner
    }
}

impl Drop for VenomShm {
    fn drop(&mut self) {
        // Unmap memory
        unsafe {
            let _ = munmap(self.addr.as_ptr().cast(), self.size);
        }

        // If owner, unlink the shared memory
        if self.is_owner {
            let full_name = format!("{}{}", VENOM_SHM_PREFIX, self.name);
            if let Ok(c_name) = CString::new(full_name) {
                let _ = shm_unlink(c_name.as_c_str());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_open() {
        let name = "test_shm_create";
        let size = 4096;

        // Create
        let shm1 = VenomShm::create(name, size).unwrap();
        assert!(shm1.is_owner());
        assert_eq!(shm1.size(), size);

        // Write some data
        unsafe {
            std::ptr::write(shm1.as_ptr(), 42u8);
        }

        // Open from another "process"
        let shm2 = VenomShm::open(name).unwrap();
        assert!(!shm2.is_owner());

        // Read the data
        let val = unsafe { std::ptr::read(shm2.as_ptr()) };
        assert_eq!(val, 42u8);

        // Drop shm2 first, then shm1 will unlink
        drop(shm2);
        drop(shm1);
    }
}
