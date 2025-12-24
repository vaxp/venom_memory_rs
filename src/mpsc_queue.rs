//! Lock-Free MPSC (Multiple Producer Single Consumer) Queue
//!
//! This queue allows multiple shells (producers) to send commands
//! to a single daemon (consumer) without locks.
//!
//! # Design
//! - Fixed-size slots with state machine
//! - Producers: atomic claim -> write -> publish
//! - Consumer: read -> process -> release

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};

/// Maximum command size in bytes
pub const MAX_CMD_SIZE: usize = 4096;

/// Maximum number of command slots
pub const MAX_SLOTS: usize = 64;

/// Cache line size
const CACHE_LINE_SIZE: usize = 64;

/// Slot states
mod slot_state {
    pub const EMPTY: u8 = 0;
    pub const WRITING: u8 = 1;
    pub const READY: u8 = 2;
    pub const PROCESSING: u8 = 3;
}

/// Padding to cache line
#[repr(C, align(64))]
struct CachePadded<T>(T);

/// A single command slot
#[repr(C)]
pub struct CommandSlot {
    /// Slot state (empty, writing, ready, processing)
    state: AtomicU8,
    /// Client ID that sent this command
    client_id: AtomicU32,
    /// Length of command data
    cmd_len: AtomicU32,
    /// Padding for alignment
    _pad: [u8; 64 - 9],
    /// Command data (separate cache line)
    cmd_data: [u8; MAX_CMD_SIZE],
}

/// MPSC Queue header in shared memory
#[repr(C)]
pub struct MpscQueueHeader {
    /// Write index (producers increment this to claim slots)
    write_idx: CachePadded<AtomicU64>,
    /// Read index (consumer's current position)
    read_idx: CachePadded<AtomicU64>,
    /// Number of slots
    num_slots: usize,
    /// Padding
    _pad: [u8; CACHE_LINE_SIZE - 8],
}

impl MpscQueueHeader {
    /// Size of the queue in bytes (header + slots)
    pub const fn size_for_slots(num_slots: usize) -> usize {
        std::mem::size_of::<MpscQueueHeader>() + num_slots * std::mem::size_of::<CommandSlot>()
    }

    /// Initialize a new queue header
    ///
    /// # Safety
    /// Pointer must be valid and properly aligned
    pub unsafe fn init(ptr: *mut Self, num_slots: usize) {
        (*ptr).write_idx.0 = AtomicU64::new(0);
        (*ptr).read_idx.0 = AtomicU64::new(0);
        (*ptr).num_slots = num_slots;

        // Initialize all slots to empty
        let slots_ptr = (ptr as *mut u8).add(std::mem::size_of::<MpscQueueHeader>())
            as *mut CommandSlot;
        for i in 0..num_slots {
            let slot = &mut *slots_ptr.add(i);
            slot.state = AtomicU8::new(slot_state::EMPTY);
            slot.client_id = AtomicU32::new(0);
            slot.cmd_len = AtomicU32::new(0);
        }
    }
}

/// Producer handle for sending commands
pub struct MpscProducer {
    header: *const MpscQueueHeader,
    slots: *mut CommandSlot,
    client_id: u32,
}

// SAFETY: Producers use atomic operations for thread safety
unsafe impl Send for MpscProducer {}

impl MpscProducer {
    /// Create a producer from raw pointers
    ///
    /// # Safety
    /// Pointers must be valid and point to initialized queue
    pub unsafe fn from_raw(header: *const MpscQueueHeader, client_id: u32) -> Self {
        let slots = (header as *mut u8).add(std::mem::size_of::<MpscQueueHeader>())
            as *mut CommandSlot;
        Self {
            header,
            slots,
            client_id,
        }
    }

    /// Try to push a command (non-blocking)
    ///
    /// Returns `true` if successful, `false` if queue is full
    #[inline]
    pub fn try_push(&self, cmd: &[u8]) -> bool {
        if cmd.len() > MAX_CMD_SIZE {
            return false;
        }

        let header = unsafe { &*self.header };
        let num_slots = header.num_slots;

        // Claim a slot
        let idx = header.write_idx.0.fetch_add(1, Ordering::AcqRel);
        let slot_idx = (idx as usize) % num_slots;
        let slot = unsafe { &*self.slots.add(slot_idx) };

        // Try to transition: EMPTY -> WRITING
        match slot.state.compare_exchange(
            slot_state::EMPTY,
            slot_state::WRITING,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                // Write client ID and data
                slot.client_id.store(self.client_id, Ordering::Relaxed);
                slot.cmd_len.store(cmd.len() as u32, Ordering::Relaxed);

                unsafe {
                    let slot_ptr = self.slots.add(slot_idx);
                    std::ptr::copy_nonoverlapping(
                        cmd.as_ptr(),
                        (*slot_ptr).cmd_data.as_mut_ptr(),
                        cmd.len(),
                    );
                }

                // Publish: WRITING -> READY
                slot.state.store(slot_state::READY, Ordering::Release);
                true
            }
            Err(_) => {
                // Slot not empty, queue might be full
                false
            }
        }
    }

    /// Push a command, spinning until space is available
    #[inline]
    pub fn push(&self, cmd: &[u8]) {
        while !self.try_push(cmd) {
            core::hint::spin_loop();
        }
    }
}

/// Consumer handle for receiving commands
pub struct MpscConsumer {
    header: *const MpscQueueHeader,
    slots: *mut CommandSlot,
}

// SAFETY: Only one consumer should exist
unsafe impl Send for MpscConsumer {}

impl MpscConsumer {
    /// Create a consumer from raw pointer
    ///
    /// # Safety
    /// Pointer must be valid and only one consumer should exist
    pub unsafe fn from_raw(header: *const MpscQueueHeader) -> Self {
        let slots = (header as *mut u8).add(std::mem::size_of::<MpscQueueHeader>())
            as *mut CommandSlot;
        Self { header, slots }
    }

    /// Try to pop a command (non-blocking)
    ///
    /// Returns `Some((client_id, data_len))` if a command was read
    /// The data is copied into the provided buffer
    #[inline]
    pub fn try_pop(&self, buf: &mut [u8]) -> Option<(u32, usize)> {
        let header = unsafe { &*self.header };
        let num_slots = header.num_slots;

        let read_idx = header.read_idx.0.load(Ordering::Acquire);
        let slot_idx = (read_idx as usize) % num_slots;
        let slot = unsafe { &*self.slots.add(slot_idx) };

        // Check if slot is ready
        if slot.state.load(Ordering::Acquire) != slot_state::READY {
            return None;
        }

        // Mark as processing
        slot.state.store(slot_state::PROCESSING, Ordering::Release);

        // Read data
        let client_id = slot.client_id.load(Ordering::Relaxed);
        let cmd_len = slot.cmd_len.load(Ordering::Relaxed) as usize;
        let copy_len = cmd_len.min(buf.len());

        unsafe {
            std::ptr::copy_nonoverlapping(
                slot.cmd_data.as_ptr(),
                buf.as_mut_ptr(),
                copy_len,
            );
        }

        // Release slot: PROCESSING -> EMPTY
        slot.state.store(slot_state::EMPTY, Ordering::Release);

        // Advance read index
        header.read_idx.0.fetch_add(1, Ordering::Release);

        Some((client_id, cmd_len))
    }

    /// Pop a command, spinning until one is available
    #[inline]
    pub fn pop(&self, buf: &mut [u8]) -> (u32, usize) {
        loop {
            if let Some(result) = self.try_pop(buf) {
                return result;
            }
            core::hint::spin_loop();
        }
    }

    /// Pop with a maximum number of spins, then return None
    #[inline]
    pub fn pop_with_spins(&self, buf: &mut [u8], max_spins: u32) -> Option<(u32, usize)> {
        for _ in 0..max_spins {
            if let Some(result) = self.try_pop(buf) {
                return Some(result);
            }
            core::hint::spin_loop();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpsc_basic() {
        let num_slots = 16;
        let size = MpscQueueHeader::size_for_slots(num_slots);

        let layout = std::alloc::Layout::from_size_align(size, 64).unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        let header = ptr as *mut MpscQueueHeader;

        unsafe {
            MpscQueueHeader::init(header, num_slots);
        }

        let producer = unsafe { MpscProducer::from_raw(header, 1) };
        let consumer = unsafe { MpscConsumer::from_raw(header) };

        // Push a command
        let cmd = b"test command";
        assert!(producer.try_push(cmd));

        // Pop it
        let mut buf = [0u8; 256];
        let result = consumer.try_pop(&mut buf);
        assert!(result.is_some());

        let (client_id, len) = result.unwrap();
        assert_eq!(client_id, 1);
        assert_eq!(len, cmd.len());
        assert_eq!(&buf[..len], cmd);

        unsafe {
            std::alloc::dealloc(ptr, layout);
        }
    }
}
