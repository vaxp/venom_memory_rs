//! Flutter/Dart Templates for VenomMemory projects
//!
//! Generates a complete Flutter project with:
//! - lib/venom_binding.dart - FFI bindings and state classes  
//! - lib/venom_shell.dart - Shell wrapper with library loading
//! - pubspec.yaml - Package configuration
//! - README.md with usage instructions

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    // Dart project structure:
    // - lib/ for library code (venom_binding.dart)
    // - bin/ for executables (main.dart)
    // - native/ for the bundled .so library
    crate::create_dir(&format!("{}/lib", base));
    crate::create_dir(&format!("{}/bin", base));
    crate::create_dir(&format!("{}/native", base));
    crate::create_dir(&format!("{}/daemon/src", base));
    
    // Dart client files
    let snake = config.name.replace("-", "_");
    crate::write_file(&format!("{}/lib/venom_binding.dart", base), &venom_binding(config));
    crate::write_file(&format!("{}/bin/{}.dart", base, snake), &main_dart(config));
    crate::write_file(&format!("{}/pubspec.yaml", base), &pubspec(config));
    
    // C Daemon files (so Flutter project is self-contained)
    crate::write_file(&format!("{}/daemon/src/main.c", base), &daemon_c(config));
    crate::write_file(&format!("{}/daemon/Makefile", base), &daemon_makefile(config));
    crate::write_file(&format!("{}/daemon/protocol.h", base), &protocol_h(config));
    
    crate::write_file(&format!("{}/README.md", base), &readme(config));
    
    // Copy the bundled library to native/ folder (for Dart) and daemon/ (for C daemon)
    let native_dir = format!("{}/native", base);
    let lib_path = format!("{}/libvenom_memory.so", native_dir);
    std::fs::write(&lib_path, crate::library::LIBRARY_BINARY)
        .expect(&format!("Failed to write library to: {}", lib_path));
    
    // Also copy to daemon folder
    let daemon_lib_path = format!("{}/daemon/libvenom_memory.so", base);
    std::fs::write(&daemon_lib_path, crate::library::LIBRARY_BINARY)
        .expect(&format!("Failed to write library to: {}", daemon_lib_path));
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in &[&lib_path, &daemon_lib_path] {
            if let Ok(meta) = std::fs::metadata(path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(path, perms).ok();
            }
        }
    }
    
    println!("   {} {}", console::style("✓").green(), lib_path);
    println!("   {} {}", console::style("✓").green(), daemon_lib_path);
}

fn magic(channel: &str) -> u32 {
    channel.bytes().fold(0x564E4Fu32, |acc, b| acc.wrapping_add(b as u32))
}

fn upper_name(name: &str) -> String {
    name.to_uppercase().replace("-", "_")
}

// ═══════════════════════════════════════════════════════════════════════════
// C Daemon (so Flutter project is self-contained)
// ═══════════════════════════════════════════════════════════════════════════

fn protocol_h(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"#ifndef {upper}_PROTOCOL_H
#define {upper}_PROTOCOL_H

#include <stdint.h>
#define {upper}_CHANNEL_NAME "{channel}"
#define {upper}_MAGIC 0x{magic:08X}
#define {upper}_MAX_CORES 16

typedef struct __attribute__((packed)) {{
    uint32_t magic;
    uint32_t version;
    float cpu_usage_percent;
    float cpu_cores[{upper}_MAX_CORES];
    uint32_t core_count;
    uint32_t memory_used_mb;
    uint32_t memory_total_mb;
    uint64_t uptime_seconds;
    uint64_t update_counter;
    uint64_t timestamp_ns;
}} {pascal}State;

#endif
"#,
        upper = upper,
        pascal = pascal,
        channel = config.channel,
        magic = magic(&config.channel)
    )
}

fn daemon_c(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"/* {name} Daemon - VenomMemory */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
#include <time.h>
#include "../protocol.h"

typedef struct VenomDaemonHandle VenomDaemonHandle;
typedef struct {{ size_t data_size; size_t cmd_slots; size_t max_clients; }} VenomConfig;
extern VenomDaemonHandle* venom_daemon_create(const char* name, VenomConfig config);
extern void venom_daemon_destroy(VenomDaemonHandle* handle);
extern void venom_daemon_write_data(VenomDaemonHandle* handle, const uint8_t* data, size_t len);

static VenomDaemonHandle* g_daemon = NULL;
static {pascal}State g_state = {{0}};
static volatile int g_running = 1;
static uint64_t prev_total[{upper}_MAX_CORES + 1] = {{0}};
static uint64_t prev_idle[{upper}_MAX_CORES + 1] = {{0}};

static void signal_handler(int sig) {{ (void)sig; g_running = 0; }}

static void read_cpu(void) {{
    FILE* f = fopen("/proc/stat", "r");
    if (!f) return;
    char line[256];
    int idx = 0;
    while (fgets(line, sizeof(line), f) && idx <= {upper}_MAX_CORES) {{
        if (strncmp(line, "cpu", 3) != 0) continue;
        uint64_t user, nice, system, idle, iowait, irq, softirq;
        if (sscanf(line + (line[3] == ' ' ? 4 : 5), "%lu %lu %lu %lu %lu %lu %lu",
                   &user, &nice, &system, &idle, &iowait, &irq, &softirq) != 7) continue;
        uint64_t total = user + nice + system + idle + iowait + irq + softirq;
        uint64_t idle_t = idle + iowait;
        uint64_t td = total - prev_total[idx], id = idle_t - prev_idle[idx];
        float usage = td > 0 ? (1.0f - (float)id / (float)td) * 100.0f : 0;
        if (line[3] == ' ') g_state.cpu_usage_percent = usage;
        else if (idx > 0 && idx <= {upper}_MAX_CORES) g_state.cpu_cores[idx-1] = usage;
        prev_total[idx] = total; prev_idle[idx] = idle_t; idx++;
    }}
    g_state.core_count = idx > 1 ? idx - 1 : 0;
    fclose(f);
}}

static void read_mem(void) {{
    FILE* f = fopen("/proc/meminfo", "r");
    if (!f) return;
    char line[256];
    uint64_t total = 0, avail = 0;
    while (fgets(line, sizeof(line), f)) {{
        if (strncmp(line, "MemTotal:", 9) == 0) sscanf(line + 9, "%lu", &total);
        else if (strncmp(line, "MemAvailable:", 13) == 0) sscanf(line + 13, "%lu", &avail);
    }}
    g_state.memory_total_mb = (uint32_t)(total / 1024);
    g_state.memory_used_mb = (uint32_t)((total - avail) / 1024);
    fclose(f);
}}

static void read_uptime(void) {{
    FILE* f = fopen("/proc/uptime", "r");
    if (!f) return;
    double up; if (fscanf(f, "%lf", &up) == 1) g_state.uptime_seconds = (uint64_t)up;
    fclose(f);
}}

int main(void) {{
    printf("🖥️  {name} Daemon (VenomMemory)\\n");
    printf("═══════════════════════════════════════════════════════════════\\n");
    signal(SIGINT, signal_handler); signal(SIGTERM, signal_handler);
    
    VenomConfig cfg = {{ .data_size = 16384, .cmd_slots = 32, .max_clients = 16 }};
    g_daemon = venom_daemon_create({upper}_CHANNEL_NAME, cfg);
    if (!g_daemon) {{ printf("❌ Failed to create channel\\n"); return 1; }}
    
    printf("✅ Channel: %s\\n🚀 Publishing... (Ctrl+C to stop)\\n\\n", {upper}_CHANNEL_NAME);
    
    while (g_running) {{
        read_cpu(); read_mem(); read_uptime();
        g_state.magic = {upper}_MAGIC; g_state.version = 1; g_state.update_counter++;
        struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts);
        g_state.timestamp_ns = (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
        venom_daemon_write_data(g_daemon, (const uint8_t*)&g_state, sizeof(g_state));
        printf("\\r🖥️  CPU: %5.1f%% | RAM: %u/%u MB | #%lu   ",
            g_state.cpu_usage_percent, g_state.memory_used_mb, g_state.memory_total_mb,
            (unsigned long)g_state.update_counter);
        fflush(stdout); usleep(100000);
    }}
    venom_daemon_destroy(g_daemon);
    printf("\\n\\n👋 Goodbye!\\n");
    return 0;
}}
"#, name = config.name, upper = upper, pascal = pascal)
}

fn daemon_makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Daemon Makefile

CC = gcc
CFLAGS = -Wall -Wextra -O2
LDFLAGS = -L. -lvenom_memory -Wl,-rpath,'$$ORIGIN'

TARGET = {name}_daemon

.PHONY: all clean run

all: $(TARGET)

$(TARGET): src/main.c
	@echo "🔗 Building $(TARGET)..."
	@$(CC) $(CFLAGS) src/main.c -o $(TARGET) $(LDFLAGS)
	@echo "✅ Build complete"

clean:
	@rm -f $(TARGET)

run: $(TARGET)
	@./$(TARGET)
"#, name = config.name)
}

fn pascal_case(s: &str) -> String {
    s.split(|c| c == '_' || c == '-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

fn venom_binding(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    let snake = config.name.replace("-", "_");
    
    format!(r#"/// VenomMemory FFI Bindings for {name}
/// 
/// Provides:
/// - {pascal}State: System stats from daemon
/// - VenomShell: Connection to daemon
///
/// Library location: native/libvenom_memory.so

import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';
import 'package:ffi/ffi.dart';

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

const String channelName = '{channel}';
const int magic = 0x{magic:08X};
const int maxCores = 16;

// ═══════════════════════════════════════════════════════════════════════════
// State Structure (matches C struct layout exactly)
// ═══════════════════════════════════════════════════════════════════════════

class {pascal}State {{
  final int magic;
  final int version;
  final double cpuUsage;
  final List<double> cpuCores;
  final int coreCount;
  final int memoryUsedMb;
  final int memoryTotalMb;
  final int uptimeSeconds;
  final int updateCounter;
  final int timestampNs;

  {pascal}State({{
    required this.magic,
    required this.version,
    required this.cpuUsage,
    required this.cpuCores,
    required this.coreCount,
    required this.memoryUsedMb,
    required this.memoryTotalMb,
    required this.uptimeSeconds,
    required this.updateCounter,
    required this.timestampNs,
  }});

  /// Parse state from raw bytes (must match C struct layout)
  factory {pascal}State.fromBytes(Uint8List bytes) {{
    if (bytes.length < 112) return {pascal}State.empty();
    
    final data = ByteData.view(bytes.buffer, bytes.offsetInBytes, bytes.length);
    
    // Parse per-core CPU usage (16 floats starting at offset 12)
    final cores = <double>[];
    for (int i = 0; i < maxCores; i++) {{
      cores.add(data.getFloat32(12 + i * 4, Endian.little));
    }}
    
    return {pascal}State(
      magic: data.getUint32(0, Endian.little),
      version: data.getUint32(4, Endian.little),
      cpuUsage: data.getFloat32(8, Endian.little),
      cpuCores: cores,
      coreCount: data.getUint32(76, Endian.little),
      memoryUsedMb: data.getUint32(80, Endian.little),
      memoryTotalMb: data.getUint32(84, Endian.little),
      uptimeSeconds: data.getUint64(88, Endian.little),
      updateCounter: data.getUint64(96, Endian.little),
      timestampNs: data.getUint64(104, Endian.little),
    );
  }}

  factory {pascal}State.empty() => {pascal}State(
    magic: 0, version: 0, cpuUsage: 0, cpuCores: List.filled(maxCores, 0.0),
    coreCount: 0, memoryUsedMb: 0, memoryTotalMb: 0, 
    uptimeSeconds: 0, updateCounter: 0, timestampNs: 0,
  );

  bool get isValid => magic == {snake}Magic;
  
  double get memoryUsagePercent => 
    memoryTotalMb > 0 ? memoryUsedMb / memoryTotalMb * 100 : 0;
  
  String get uptimeFormatted {{
    final hours = uptimeSeconds ~/ 3600;
    final minutes = (uptimeSeconds % 3600) ~/ 60;
    return '${{hours}}h ${{minutes}}m';
  }}
}}

const int {snake}Magic = magic;

// ═══════════════════════════════════════════════════════════════════════════
// VenomShell - Connection to VenomMemory Daemon
// ═══════════════════════════════════════════════════════════════════════════

/// Finds the native library in various possible locations
String _findLibraryPath() {{
  // List of possible locations to search
  final locations = [
    // Relative to executable (for deployed apps)
    'native/libvenom_memory.so',
    '../native/libvenom_memory.so',
    '../../native/libvenom_memory.so',
    // Standard lib location
    'lib/libvenom_memory.so',
    '../lib/libvenom_memory.so', 
    // Absolute fallback
    '/usr/local/lib/libvenom_memory.so',
    '/usr/lib/libvenom_memory.so',
  ];
  
  for (final path in locations) {{
    if (File(path).existsSync()) {{
      return path;
    }}
  }}
  
  // Try from current directory
  final cwd = Directory.current.path;
  for (final path in locations) {{
    final fullPath = '$cwd/$path';
    if (File(fullPath).existsSync()) {{
      return fullPath;
    }}
  }}
  
  throw Exception(
    'Could not find libvenom_memory.so. Searched in:\n'
    '${{locations.join("\n")}}\n\n'
    'Make sure native/libvenom_memory.so exists in your project.'
  );
}}

class VenomShell {{
  static DynamicLibrary? _lib;
  Pointer<Void>? _handle;
  bool _disposed = false;

  VenomShell() {{
    // Load library from native/ directory
    final libPath = _findLibraryPath();
    _lib ??= DynamicLibrary.open(libPath);
    
    // Connect to channel
    final connect = _lib!.lookupFunction<
      Pointer<Void> Function(Pointer<Utf8>),
      Pointer<Void> Function(Pointer<Utf8>)
    >('venom_shell_connect');
    
    final namePtr = channelName.toNativeUtf8();
    _handle = connect(namePtr);
    calloc.free(namePtr);
    
    if (_handle == nullptr) {{
      throw Exception('Failed to connect to channel "$channelName". Is the daemon running?');
    }}
  }}

  /// Get the client ID assigned by the daemon
  int get clientId {{
    _checkDisposed();
    final fn = _lib!.lookupFunction<
      Uint32 Function(Pointer<Void>), 
      int Function(Pointer<Void>)
    >('venom_shell_id');
    return fn(_handle!);
  }}

  /// Read raw data from shared memory
  Uint8List readRawData(int maxLen) {{
    _checkDisposed();
    final fn = _lib!.lookupFunction<
      IntPtr Function(Pointer<Void>, Pointer<Uint8>, IntPtr), 
      int Function(Pointer<Void>, Pointer<Uint8>, int)
    >('venom_shell_read_data');
    
    final buf = calloc<Uint8>(maxLen);
    try {{
      final len = fn(_handle!, buf, maxLen);
      return Uint8List.fromList(buf.asTypedList(len));
    }} finally {{
      calloc.free(buf);
    }}
  }}

  /// Read and parse state from daemon
  {pascal}State readState() {{
    final bytes = readRawData(256);
    return {pascal}State.fromBytes(bytes);
  }}

  /// Clean up resources
  void dispose() {{
    if (_disposed) return;
    _disposed = true;
    
    final fn = _lib!.lookupFunction<
      Void Function(Pointer<Void>), 
      void Function(Pointer<Void>)
    >('venom_shell_destroy');
    fn(_handle!);
    _handle = null;
  }}
  
  void _checkDisposed() {{
    if (_disposed) throw StateError('VenomShell has been disposed');
  }}
}}
"#,
        name = config.name,
        channel = config.channel,
        magic = magic(&config.channel),
        pascal = pascal,
        snake = snake
    )
}

fn main_dart(config: &ProjectConfig) -> String {
    let snake = config.name.replace("-", "_");
    
    format!(r#"/// {name} - VenomMemory Client Example - with Benchmarking
/// 
/// Demonstrates connecting to daemon and reading system stats.
/// Includes read latency measurements.

import 'dart:io';
import 'package:{snake}/venom_binding.dart';

// ANSI colors
const cyan = '\x1B[96m';
const reset = '\x1B[0m';

void main() async {{
  print('🖥️  {name} Client (Flutter/Dart)');
  print('═══════════════════════════════════════════════════════════════');
  
  // Latency tracking
  var latencyMin = double.maxFinite;
  var latencyMax = 0.0;
  var latencySum = 0.0;
  var latencyCount = 0;
  var frame = 0;
  
  try {{
    final shell = VenomShell();
    print('✅ Connected! Client ID: ${{shell.clientId}}');
    print('📊 Reading system stats... (Ctrl+C to exit)\n');
    
    // Handle Ctrl+C for final stats
    ProcessSignal.sigint.watch().listen((_) {{
      print('\n');
      print('📊 ${{cyan}}Final Latency Stats (Flutter/Dart):${{reset}}');
      print('   Samples: $latencyCount');
      print('   Min: ${{latencyMin.toStringAsFixed(2)}} µs');
      print('   Max: ${{latencyMax.toStringAsFixed(2)}} µs');
      print('   Avg: ${{(latencySum / latencyCount).toStringAsFixed(2)}} µs');
      print('\n👋 Goodbye!');
      exit(0);
    }});
    
    while (true) {{
      // ═══════════════════════════════════════════════════════════════════
      // 📊 BENCHMARK: Measure read latency
      // ═══════════════════════════════════════════════════════════════════
      final stopwatch = Stopwatch()..start();
      final state = shell.readState();
      stopwatch.stop();
      final latencyUs = stopwatch.elapsedMicroseconds.toDouble();
      
      // Update stats
      if (latencyUs < latencyMin) latencyMin = latencyUs;
      if (latencyUs > latencyMax) latencyMax = latencyUs;
      latencySum += latencyUs;
      latencyCount++;
      final avgUs = latencySum / latencyCount;
      
      if (state.isValid) {{
        // Clear screen and move cursor to top
        stdout.write('\x1B[2J\x1B[H');
        
        print('╔═══════════════════════════════════════════════════════════════╗');
        print('║  🖥️  {name} Monitor (Flutter)    Frame: ${{frame.toString().padRight(6)}}         ║');
        print('╠═══════════════════════════════════════════════════════════════╣');
        print('║  CPU: ${{state.cpuUsage.toStringAsFixed(1).padLeft(5)}}%  |  '
              'RAM: ${{state.memoryUsedMb}}/${{state.memoryTotalMb}} MB  |  '
              'Uptime: ${{state.uptimeFormatted}}  ║');
        print('╠═══════════════════════════════════════════════════════════════╣');
        
        // Show per-core usage (all cores)
        for (var i = 0; i < state.coreCount; i++) {{
          final usage = state.cpuCores[i].toStringAsFixed(1).padLeft(5);
          print('║  Core $i: $usage%                                                ║');
        }}
        
        print('╠═══════════════════════════════════════════════════════════════╣');
        print('║  Memory: ${{state.memoryUsagePercent.toStringAsFixed(1)}}% used                                           ║');
        print('╠═══════════════════════════════════════════════════════════════╣');
        print('║  📊 ${{cyan}}Read Latency:${{reset}} ${{latencyUs.toStringAsFixed(2)}} µs (min: ${{latencyMin.toStringAsFixed(2)}}, max: ${{latencyMax.toStringAsFixed(2)}}, avg: ${{avgUs.toStringAsFixed(2)}})  ║');
        print('╚═══════════════════════════════════════════════════════════════╝');
        print('  Updates: ${{state.updateCounter}} | Press Ctrl+C to exit');
        frame++;
      }} else {{
        print('⏳ Waiting for valid data from daemon...');
      }}
      
      await Future.delayed(Duration(milliseconds: 100));
    }}
  }} catch (e) {{
    print('❌ Error: $e');
    print('\nMake sure:');
    print('  1. The daemon is running');
    print('  2. native/libvenom_memory.so exists');
    exit(1);
  }}
}}
"#, name = config.name, snake = snake)
}

fn pubspec(config: &ProjectConfig) -> String {
    let snake = config.name.replace("-", "_");
    format!(r#"name: {snake}
description: VenomMemory client for {name} - Real-time system monitoring via shared memory IPC

environment:
  sdk: '>=3.0.0 <4.0.0'

dependencies:
  ffi: ^2.1.0
"#, name = config.name, snake = snake)
}

fn readme(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    format!(r#"# {name} (Flutter/Dart)

VenomMemory Flutter client for real-time system monitoring.

## Project Structure

```
{name}/
├── lib/
│   ├── venom_binding.dart   # FFI bindings & {pascal}State
│   └── main.dart            # Example client
├── native/
│   └── libvenom_memory.so   # Bundled VenomMemory library
└── pubspec.yaml
```

## Quick Start

```bash
# Make sure daemon is running first!

# Run the Dart client
dart run
```

## Usage in Your Code

```dart
import 'venom_binding.dart';

void main() {{
  final shell = VenomShell();
  print('Connected! ID: ${{shell.clientId}}');

  // Read system stats
  final state = shell.readState();
  if (state.isValid) {{
    print('CPU: ${{state.cpuUsage.toStringAsFixed(1)}}%');
    print('RAM: ${{state.memoryUsedMb}}/${{state.memoryTotalMb}} MB');
    print('Uptime: ${{state.uptimeFormatted}}');
  }}

  // Don't forget to clean up!
  shell.dispose();
}}
```

## Configuration

| Setting | Value |
|---------|-------|
| Channel | `{channel}` |
| Magic | `0x{magic:08X}` |

## Notes

- The library is bundled in `native/libvenom_memory.so`
- Make sure the daemon is running before starting the client
- For Flutter mobile apps, you'll need platform-specific library setup
"#,
        name = config.name,
        channel = config.channel,
        magic = magic(&config.channel),
        pascal = pascal
    )
}
