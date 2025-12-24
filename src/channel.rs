//! High-level Channel API for VenomMemory
//!
//! Provides easy-to-use interfaces for daemon (writer) and shell (reader) processes.

use crate::error::{Result, VenomError};
use crate::mpsc_queue::{MpscConsumer, MpscProducer, MpscQueueHeader, MAX_CMD_SIZE};
use crate::seqlock::{SeqLockHeader, SeqLockReader, SeqLockWriter};
use crate::shm::VenomShm;
use std::sync::atomic::{AtomicU32, Ordering};

/// Magic number for channel validation
const VENOM_MAGIC: u32 = 0x564E4F4D; // "VNOM"
const VENOM_VERSION: u32 = 2;

/// Default data region size (64KB)
const DEFAULT_DATA_SIZE: usize = 64 * 1024;

/// Default number of command slots
const DEFAULT_CMD_SLOTS: usize = 32;

/// Cache line size
const CACHE_LINE_SIZE: usize = 64;

/// Channel configuration
#[derive(Clone)]
pub struct ChannelConfig {
    /// Size of the data region in bytes
    pub data_size: usize,
    /// Number of command queue slots
    pub cmd_slots: usize,
    /// Maximum number of clients
    pub max_clients: usize,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            data_size: DEFAULT_DATA_SIZE,
            cmd_slots: DEFAULT_CMD_SLOTS,
            max_clients: 16,
        }
    }
}

/// Channel header stored at the beginning of shared memory
#[repr(C)]
struct ChannelHeader {
    magic: u32,
    version: u32,
    data_size: usize,
    cmd_slots: usize,
    max_clients: usize,
    next_client_id: AtomicU32,
    // Offsets to regions
    seqlock_offset: usize,
    cmd_queue_offset: usize,
    _pad: [u8; CACHE_LINE_SIZE - 48],
}

impl ChannelHeader {
    fn total_size(config: &ChannelConfig) -> usize {
        let header_size = std::mem::size_of::<ChannelHeader>();
        let seqlock_size = std::mem::size_of::<SeqLockHeader>() + config.data_size;
        let cmd_queue_size = MpscQueueHeader::size_for_slots(config.cmd_slots);

        // Align each region to cache line
        let align = |size: usize| -> usize { (size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1) };

        align(header_size) + align(seqlock_size) + align(cmd_queue_size)
    }
}

/// Daemon (Writer) side of the channel
pub struct DaemonChannel {
    shm: VenomShm,
    #[allow(dead_code)]
    header: *mut ChannelHeader,
    data_writer: SeqLockWriter,
    cmd_consumer: MpscConsumer,
}

// SAFETY: DaemonChannel is designed for single-threaded use
unsafe impl Send for DaemonChannel {}

impl DaemonChannel {
    /// Create a new channel as the daemon (owner)
    pub fn create(namespace: &str, config: ChannelConfig) -> Result<Self> {
        let total_size = ChannelHeader::total_size(&config);
        let shm = VenomShm::create(namespace, total_size)?;

        let base = shm.as_ptr();
        let header = base as *mut ChannelHeader;

        // Calculate offsets
        let header_end = std::mem::size_of::<ChannelHeader>();
        let seqlock_offset = (header_end + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);
        let seqlock_size = std::mem::size_of::<SeqLockHeader>() + config.data_size;
        let cmd_queue_offset =
            seqlock_offset + ((seqlock_size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1));

        unsafe {
            // Initialize header
            (*header).magic = VENOM_MAGIC;
            (*header).version = VENOM_VERSION;
            (*header).data_size = config.data_size;
            (*header).cmd_slots = config.cmd_slots;
            (*header).max_clients = config.max_clients;
            (*header).next_client_id = AtomicU32::new(1);
            (*header).seqlock_offset = seqlock_offset;
            (*header).cmd_queue_offset = cmd_queue_offset;

            // Initialize SeqLock
            let seqlock_header = base.add(seqlock_offset) as *mut SeqLockHeader;
            SeqLockHeader::init(seqlock_header, config.data_size);

            // Initialize command queue
            let cmd_queue_header = base.add(cmd_queue_offset) as *mut MpscQueueHeader;
            MpscQueueHeader::init(cmd_queue_header, config.cmd_slots);

            // Create writer and consumer
            let data_ptr = base.add(seqlock_offset + std::mem::size_of::<SeqLockHeader>());
            let data_writer = SeqLockWriter::from_raw(seqlock_header, data_ptr);
            let cmd_consumer = MpscConsumer::from_raw(cmd_queue_header);

            Ok(Self {
                shm,
                header,
                data_writer,
                cmd_consumer,
            })
        }
    }

    /// Write data to the shared region
    ///
    /// All connected shells will be able to read this data
    #[inline]
    pub fn write_data(&self, data: &[u8]) {
        self.data_writer.write(data);
    }

    /// Write data with length prefix (for variable-size data)
    #[inline]
    pub fn write_data_with_len(&self, data: &[u8]) {
        self.data_writer.write_with_len(data);
    }

    /// Try to receive a command from any shell
    ///
    /// Returns `Some((client_id, data_length))` if a command is available
    #[inline]
    pub fn try_recv_command(&self, buf: &mut [u8]) -> Option<(u32, usize)> {
        self.cmd_consumer.try_pop(buf)
    }

    /// Receive a command, spinning until one is available
    #[inline]
    pub fn recv_command(&self, buf: &mut [u8]) -> (u32, usize) {
        self.cmd_consumer.pop(buf)
    }

    /// Run the daemon loop with a handler function
    ///
    /// The handler receives (client_id, command) and returns the response data
    pub fn run<F>(&self, mut handler: F)
    where
        F: FnMut(u32, &[u8]) -> Vec<u8>,
    {
        let mut cmd_buf = [0u8; MAX_CMD_SIZE];

        loop {
            let (client_id, cmd_len) = self.recv_command(&mut cmd_buf);
            let cmd = &cmd_buf[..cmd_len];

            // Check for shutdown command
            if cmd == b"__SHUTDOWN__" {
                break;
            }

            // Process command
            let response = handler(client_id, cmd);

            // Write response as data (all shells can read)
            self.write_data_with_len(&response);
        }
    }

    /// Get the namespace of the channel
    pub fn namespace(&self) -> &str {
        self.shm.name()
    }

    /// Get raw pointer to shared memory base
    pub fn as_ptr(&self) -> *mut u8 {
        self.shm.as_ptr()
    }
}

/// Shell (Reader) side of the channel
pub struct ShellChannel {
    shm: VenomShm,
    #[allow(dead_code)]
    header: *const ChannelHeader,
    data_reader: SeqLockReader,
    cmd_producer: MpscProducer,
    client_id: u32,
}

// SAFETY: ShellChannel uses atomic operations
unsafe impl Send for ShellChannel {}
unsafe impl Sync for ShellChannel {}

impl ShellChannel {
    /// Connect to an existing channel as a shell (reader/command sender)
    pub fn connect(namespace: &str) -> Result<Self> {
        let shm = VenomShm::open(namespace)?;
        let base = shm.as_ptr();
        let header = base as *const ChannelHeader;

        unsafe {
            // Validate magic
            let magic = (*header).magic;
            if magic != VENOM_MAGIC {
                return Err(VenomError::InvalidMagic {
                    expected: VENOM_MAGIC,
                    got: magic,
                });
            }

            // Get client ID
            let client_id = (*header).next_client_id.fetch_add(1, Ordering::AcqRel);

            // Get offsets
            let seqlock_offset = (*header).seqlock_offset;
            let cmd_queue_offset = (*header).cmd_queue_offset;

            // Create reader and producer
            let seqlock_header = base.add(seqlock_offset) as *const SeqLockHeader;
            let data_ptr = base.add(seqlock_offset + std::mem::size_of::<SeqLockHeader>());
            let data_reader = SeqLockReader::from_raw(seqlock_header, data_ptr);

            let cmd_queue_header = base.add(cmd_queue_offset) as *const MpscQueueHeader;
            let cmd_producer = MpscProducer::from_raw(cmd_queue_header, client_id);

            Ok(Self {
                shm,
                header,
                data_reader,
                cmd_producer,
                client_id,
            })
        }
    }

    /// Get this client's ID
    #[inline]
    pub fn client_id(&self) -> u32 {
        self.client_id
    }

    /// Read data from the shared region
    ///
    /// Returns the number of bytes read
    #[inline]
    pub fn read_data(&self, buf: &mut [u8]) -> usize {
        self.data_reader.read(buf)
    }

    /// Read data with length prefix
    ///
    /// Returns the actual data length
    #[inline]
    pub fn read_data_with_len(&self, buf: &mut [u8]) -> usize {
        self.data_reader.read_with_len(buf)
    }

    /// Try to read data (non-blocking)
    #[inline]
    pub fn try_read_data(&self, buf: &mut [u8]) -> Option<usize> {
        self.data_reader.try_read(buf)
    }

    /// Send a command to the daemon
    ///
    /// Returns `true` if successful, `false` if queue is full
    #[inline]
    pub fn try_send_command(&self, cmd: &[u8]) -> bool {
        self.cmd_producer.try_push(cmd)
    }

    /// Send a command, spinning until space is available
    #[inline]
    pub fn send_command(&self, cmd: &[u8]) {
        self.cmd_producer.push(cmd)
    }

    /// Send a command and wait for response
    ///
    /// This sends the command, then spins reading the data region
    /// until a new response appears
    pub fn request(&self, cmd: &[u8], response_buf: &mut [u8]) -> usize {
        // Send command
        self.send_command(cmd);

        // Spin reading until we get a response
        // In a real implementation, you'd have per-client response slots
        loop {
            let len = self.read_data_with_len(response_buf);
            if len > 0 {
                return len;
            }
            core::hint::spin_loop();
        }
    }

    /// Get the namespace of the channel
    pub fn namespace(&self) -> &str {
        self.shm.name()
    }

    /// Get raw pointer to shared memory base
    pub fn as_ptr(&self) -> *const u8 {
        self.shm.as_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_channel_create_connect() {
        let namespace = "test_channel";
        let config = ChannelConfig::default();

        // Create daemon
        let daemon = DaemonChannel::create(namespace, config).unwrap();

        // Connect shell
        let shell = ShellChannel::connect(namespace).unwrap();

        assert_eq!(shell.client_id(), 1);

        // Write data from daemon
        daemon.write_data(b"Hello from daemon!");

        // Read from shell
        let mut buf = [0u8; 256];
        let len = shell.read_data(&mut buf);
        assert!(len >= 18);

        drop(shell);
        drop(daemon);
    }
}
