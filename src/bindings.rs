//! C Bindings for VenomMemory
//!
//! Provides a raw C API for creating and connecting to channels.

use crate::channel::{ChannelConfig, DaemonChannel, ShellChannel};
use crate::error::Result;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::slice;
use std::ptr;

// Opaque handles
pub struct VenomDaemonHandle(DaemonChannel);
pub struct VenomShellHandle(ShellChannel);

#[repr(C)]
pub struct VenomConfig {
    pub data_size: usize,
    pub cmd_slots: usize,
    pub max_clients: usize,
}

/// Create a new daemon channel
///
/// # Safety
/// name must be a valid null-terminated string
#[no_mangle]
pub unsafe extern "C" fn venom_daemon_create(
    name: *const c_char,
    config: VenomConfig,
) -> *mut VenomDaemonHandle {
    if name.is_null() {
        return ptr::null_mut();
    }

    let c_str = CStr::from_ptr(name);
    let str_slice = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let rust_config = ChannelConfig {
        data_size: config.data_size,
        cmd_slots: config.cmd_slots,
        max_clients: config.max_clients,
    };

    match DaemonChannel::create(str_slice, rust_config) {
        Ok(daemon) => Box::into_raw(Box::new(VenomDaemonHandle(daemon))),
        Err(_) => ptr::null_mut(),
    }
}

/// Destroy a daemon handle
#[no_mangle]
pub unsafe extern "C" fn venom_daemon_destroy(handle: *mut VenomDaemonHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// Daemon: Wait for command (blocking/spinning)
///
/// Returns cmd length. Writes cmd into buf and client_id into out_client_id.
#[no_mangle]
pub unsafe extern "C" fn venom_daemon_recv_command(
    handle: *mut VenomDaemonHandle,
    buf: *mut u8,
    max_len: usize,
    out_client_id: *mut u32,
) -> usize {
    let daemon = &(*handle).0;
    let slice = slice::from_raw_parts_mut(buf, max_len);
    let (client_id, len) = daemon.recv_command(slice);
    if !out_client_id.is_null() {
        *out_client_id = client_id;
    }
    len
}

/// Daemon: Try to receive command (non-blocking)
///
/// Returns cmd length if command available, 0 if no command.
/// Writes cmd into buf and client_id into out_client_id.
#[no_mangle]
pub unsafe extern "C" fn venom_daemon_try_recv_command(
    handle: *mut VenomDaemonHandle,
    buf: *mut u8,
    max_len: usize,
    out_client_id: *mut u32,
) -> usize {
    let daemon = &(*handle).0;
    let slice = slice::from_raw_parts_mut(buf, max_len);
    match daemon.try_recv_command(slice) {
        Some((client_id, len)) => {
            if !out_client_id.is_null() {
                *out_client_id = client_id;
            }
            len
        }
        None => 0,
    }
}

/// Daemon: Write data to shared memory
#[no_mangle]
pub unsafe extern "C" fn venom_daemon_write_data(
    handle: *mut VenomDaemonHandle,
    data: *const u8,
    len: usize,
) {
    let daemon = &(*handle).0;
    let slice = slice::from_raw_parts(data, len);
    daemon.write_data_with_len(slice);
}

/// Get raw pointer to shared memory (offset to data region)
/// This allows implementing custom zero-copy protocols in C
#[no_mangle]
pub unsafe extern "C" fn venom_daemon_get_shm_ptr(handle: *mut VenomDaemonHandle) -> *mut u8 {
    let daemon = &(*handle).0;
    daemon.as_ptr()
}

// --- Shell Side ---

/// Connect to an existing channel
#[no_mangle]
pub unsafe extern "C" fn venom_shell_connect(name: *const c_char) -> *mut VenomShellHandle {
    if name.is_null() {
        return ptr::null_mut();
    }

    let c_str = CStr::from_ptr(name);
    let str_slice = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    match ShellChannel::connect(str_slice) {
        Ok(shell) => Box::into_raw(Box::new(VenomShellHandle(shell))),
        Err(_) => ptr::null_mut(),
    }
}

/// Destroy a shell handle
#[no_mangle]
pub unsafe extern "C" fn venom_shell_destroy(handle: *mut VenomShellHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// Shell: Read data from shared memory
/// Returns bytes read (actual data length, may be larger than buffer)
#[no_mangle]
pub unsafe extern "C" fn venom_shell_read_data(
    handle: *mut VenomShellHandle,
    buf: *mut u8,
    max_len: usize,
) -> usize {
    let shell = &(*handle).0;
    let slice = slice::from_raw_parts_mut(buf, max_len);
    shell.read_data_with_len(slice)
}

/// Shell: Get Client ID
#[no_mangle]
pub unsafe extern "C" fn venom_shell_id(handle: *mut VenomShellHandle) -> u32 {
    let shell = &(*handle).0;
    shell.client_id()
}

/// Shell: Send command
#[no_mangle]
pub unsafe extern "C" fn venom_shell_send_command(
    handle: *mut VenomShellHandle,
    cmd: *const u8,
    len: usize,
) -> bool {
    let shell = &(*handle).0;
    let slice = slice::from_raw_parts(cmd, len);
    shell.try_send_command(slice)
}

/// Get raw pointer to shared memory for shell
#[no_mangle]
pub unsafe extern "C" fn venom_shell_get_shm_ptr(handle: *mut VenomShellHandle) -> *const u8 {
    let shell = &(*handle).0;
    shell.as_ptr()
}
