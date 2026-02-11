# 📚 VenomMemory Usage Guide

A comprehensive guide to using the VenomMemory library for inter-process communication (IPC).

---

## 🎯 Overview

VenomMemory is a high-performance IPC library using shared memory. It is based on the **Daemon-Shell** model:

| Component | Role | Processes |
|-----------|------|-----------|
| **Daemon** | Server/Writer | Create channel, write data, receive commands |
| **Shell** | Client/Reader | Connect to channel, read data, send commands |

```
┌─────────────────────────────────────────────────────────────┐
│                        DAEMON                               │
│  ┌─────────────────┐    ┌─────────────────┐                │
│  │  write_data()   │──▶│   try_recv()    │◀─ Commands    │
│  └─────────────────┘    └─────────────────┘                │
└─────────────────────────────┬───────────────────────────────┘
                              │ Shared Memory
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌─────────────────────┐         ┌─────────────────────┐
│       SHELL 1       │         │       SHELL 2       │
│  read_data()        │         │  read_data()        │
│  send_command()     │         │  send_command()     │
└─────────────────────┘         └─────────────────────┘
```

---

## 📦 Installation

### Cargo.toml (Rust)
```toml
[dependencies]
venom_memory = { path = "../venom_memory_rs" }
```

### C/C++
```bash
# Copy files
cp target/release/libvenom_memory.so /usr/local/lib/
cp venom_memory_rs.h /usr/local/include/

# Link
gcc -o myapp myapp.c -lvenom_memory
```

### Flutter/Dart
```yaml
# pubspec.yaml
dependencies:
  ffi: ^2.1.0
```

---

## 🔧 Basic Usage (Rust)

### 1️⃣ Create Daemon (Server)

```rust
use venom_memory::{DaemonChannel, ChannelConfig};

fn main() {
    // Configure the channel
    let config = ChannelConfig {
        data_size: 1024,      // Data size (bytes)
        cmd_slots: 16,        // Number of command slots
        max_clients: 8,       // Maximum number of clients
    };

    // Create the channel
    let daemon = DaemonChannel::create("my_channel", config)
        .expect("Failed to create channel");

    println!("✅ Channel created: my_channel");

    loop {
        // Write data
        let data = b"Hello from daemon!";
        daemon.write_data(data);

        // Receive commands (non-blocking)
        let mut cmd_buf = [0u8; 64];
        if let Some((client_id, len)) = daemon.try_recv_command(&mut cmd_buf) {
            let cmd = String::from_utf8_lossy(&cmd_buf[..len]);
            println!("📥 Command from client {}: {}", client_id, cmd);
            
            // Handle command
            match cmd.as_ref() {
                "PING" => println!("PONG!"),
                "STOP" => break,
                _ => println!("Unknown command"),
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
```

### 2️⃣ Create Shell (Client)

```rust
use venom_memory::ShellChannel;

fn main() {
    // Connect to the channel
    let shell = ShellChannel::connect("my_channel")
        .expect("Failed to connect");

    println!("✅ Connected! Client ID: {}", shell.client_id());

    // Send command to server
    shell.try_send_command(b"PING");
    println!("📤 PING sent");

    // Read data
    let mut buf = vec![0u8; 1024];
    loop {
        let len = shell.read_data(&mut buf);
        if len > 0 {
            let data = String::from_utf8_lossy(&buf[..len]);
            println!("📥 Data: {}", data);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
```

---

## 📊 Transferring Structured Data (Structs)

### Define Shared Struct

```rust
// Must be identical on server and client!
#[repr(C)]  // Very important for C compatibility
#[derive(Clone, Copy, Default)]
pub struct SensorData {
    pub temperature: f32,
    pub humidity: f32,
    pub pressure: f32,
    pub timestamp: u64,
}
```

### Writing (Daemon)

```rust
let data = SensorData {
    temperature: 25.5,
    humidity: 60.0,
    pressure: 1013.25,
    timestamp: 1234567890,
};

// Convert struct to bytes
let bytes = unsafe {
    std::slice::from_raw_parts(
        &data as *const SensorData as *const u8,
        std::mem::size_of::<SensorData>()
    )
};

daemon.write_data(bytes);
```

### Reading (Shell)

```rust
let mut buf = vec![0u8; std::mem::size_of::<SensorData>() + 64];
let len = shell.read_data(&mut buf);

if len >= std::mem::size_of::<SensorData>() {
    let data: SensorData = unsafe {
        std::ptr::read(buf.as_ptr() as *const SensorData)
    };
    println!("🌡️ Temperature: {}°C", data.temperature);
}
```

---

## 🔌 Usage from C

### Header (venom_memory_rs.h)

```c
// Types
typedef struct VenomDaemonHandle VenomDaemonHandle;
typedef struct VenomShellHandle VenomShellHandle;

typedef struct {
    size_t data_size;
    size_t cmd_slots;
    size_t max_clients;
} VenomConfig;

// Daemon functions
VenomDaemonHandle* venom_daemon_create(const char* name, VenomConfig config);
void venom_daemon_destroy(VenomDaemonHandle* handle);
void venom_daemon_write_data(VenomDaemonHandle* handle, const uint8_t* data, size_t len);

// Client functions
VenomShellHandle* venom_shell_connect(const char* name);
void venom_shell_destroy(VenomShellHandle* handle);
size_t venom_shell_read_data(VenomShellHandle* handle, uint8_t* buf, size_t max_len);
uint32_t venom_shell_id(VenomShellHandle* handle);
bool venom_shell_send_command(VenomShellHandle* handle, const uint8_t* cmd, size_t len);
```

### C Example

```c
#include <stdio.h>
#include "venom_memory_rs.h"

int main() {
    // Connect
    VenomShellHandle* shell = venom_shell_connect("my_channel");
    if (!shell) {
        printf("❌ Connection failed\n");
        return 1;
    }
    
    printf("✅ Connected! ID: %u\n", venom_shell_id(shell));
    
    // Send command
    venom_shell_send_command(shell, (uint8_t*)"PING", 4);
    
    // Read
    uint8_t buf[1024];
    size_t len = venom_shell_read_data(shell, buf, sizeof(buf));
    printf("📥 Received %zu bytes\n", len);
    
    venom_shell_destroy(shell);
    return 0;
}
```

---

## 📱 Usage from Flutter/Dart

### venom_memory.dart

```dart
import 'dart:ffi';
import 'package:ffi/ffi.dart';

class VenomShell {
  static DynamicLibrary? _lib;
  Pointer<Void>? _handle;
  
  VenomShell(String channelName) {
    _lib ??= DynamicLibrary.open('libvenom_memory.so');
    
    final connect = _lib!.lookupFunction<
      Pointer<Void> Function(Pointer<Utf8>),
      Pointer<Void> Function(Pointer<Utf8>)
    >('venom_shell_connect');
    
    final namePtr = channelName.toNativeUtf8();
    _handle = connect(namePtr);
    calloc.free(namePtr);
  }
  
  Uint8List readData(int maxLen) {
    final readFn = _lib!.lookupFunction<
      IntPtr Function(Pointer<Void>, Pointer<Uint8>, IntPtr),
      int Function(Pointer<Void>, Pointer<Uint8>, int)
    >('venom_shell_read_data');
    
    final bufPtr = calloc<Uint8>(maxLen);
    final len = readFn(_handle!, bufPtr, maxLen);
    final result = Uint8List.fromList(bufPtr.asTypedList(len));
    calloc.free(bufPtr);
    return result;
  }
  
  bool sendCommand(String cmd) {
    final sendFn = _lib!.lookupFunction<
      Uint8 Function(Pointer<Void>, Pointer<Uint8>, IntPtr),
      int Function(Pointer<Void>, Pointer<Uint8>, int)
    >('venom_shell_send_command');
    
    final cmdBytes = cmd.codeUnits;
    final cmdPtr = calloc<Uint8>(cmdBytes.length);
    for (int i = 0; i < cmdBytes.length; i++) {
      cmdPtr[i] = cmdBytes[i];
    }
    final result = sendFn(_handle!, cmdPtr, cmdBytes.length);
    calloc.free(cmdPtr);
    return result != 0;
  }
  
  void dispose() {
    final destroy = _lib!.lookupFunction<
      Void Function(Pointer<Void>),
      void Function(Pointer<Void>)
    >('venom_shell_destroy');
    destroy(_handle!);
  }
}
```

### Usage in Flutter Widget

```dart
class SensorWidget extends StatefulWidget {
  @override
  _SensorWidgetState createState() => _SensorWidgetState();
}

class _SensorWidgetState extends State<SensorWidget> {
  late VenomShell _shell;
  double _temperature = 0;
  
  @override
  void initState() {
    super.initState();
    _shell = VenomShell('sensor_data');
    _startPolling();
  }
  
  void _startPolling() {
    Timer.periodic(Duration(milliseconds: 100), (_) {
      final bytes = _shell.readData(64);
      if (bytes.length >= 4) {
        final data = ByteData.view(bytes.buffer);
        setState(() {
          _temperature = data.getFloat32(0, Endian.little);
        });
      }
    });
  }
  
  void _sendCommand() {
    _shell.sendCommand('CALIBRATE');
  }
  
  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Text('🌡️ $_temperature°C'),
        ElevatedButton(
          onPressed: _sendCommand,
          child: Text('Calibrate'),
        ),
      ],
    );
  }
}
```

---

## 📝 API Reference

### DaemonChannel

| Function | Description |
|----------|-------------|
| `create(name, config)` | Create a new channel |
| `write_data(bytes)` | Write data (read by all shells) |
| `try_recv_command(buf)` | Receive command (non-blocking) |
| `as_ptr()` | Raw memory pointer |

### ShellChannel

| Function | Description |
|----------|-------------|
| `connect(name)` | Connect to existing channel |
| `read_data(buf)` | Read data from server |
| `try_send_command(bytes)` | Send command to server |
| `client_id()` | Unique client ID |
| `as_ptr()` | Raw memory pointer |

### ChannelConfig

| Field | Type | Description |
|-------|------|-------------|
| `data_size` | `usize` | Data area size |
| `cmd_slots` | `usize` | Number of command slots |
| `max_clients` | `usize` | Maximum number of clients |

---

## ⚠️ Important Notes

### 1. Struct Alignment
```rust
// Server and client must use the same struct!
#[repr(C)]  // Mandatory for compatibility
struct MyData {
    field1: f32,  // Same order
    field2: u32,  // Same types
}
```

### 2. Error Handling
```rust
// Always check for successful connection
let shell = match ShellChannel::connect("channel") {
    Ok(s) => s,
    Err(e) => {
        eprintln!("Connection failed: {:?}", e);
        return;
    }
};
```

### 3. Resource Cleanup
```rust
// Resources are automatically freed in Rust (Drop)
// In C you must call destroy:
venom_shell_destroy(shell);
```

### 4. Thread Safety
```rust
// VenomMemory is thread-safe
// Shell can be shared between multiple threads
let shell = Arc::new(shell);
```

---

## 🚀 Best Practices

1. **Use `#[repr(C)]`** for all shared structs
2. **Check data size** before reading
3. **Don't block the daemon** - use `try_recv_command`
4. **Choose appropriate sizes** for `data_size` and `cmd_slots`
5. **Shutdown the daemon** safely on exit

---

## 📊 Expected Performance

| Metric | Value |
|--------|-------|
| Bandwidth | ~40 GB/s |
| Latency | ~50 µs |
| syscalls | 0 (after creation) |

---

## 🔗 Useful Links

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) - Technical architecture
- [examples/system_daemon.rs](../examples/system_daemon.rs) - Complete example
- [examples/status_bar.rs](../examples/status_bar.rs) - Client example
