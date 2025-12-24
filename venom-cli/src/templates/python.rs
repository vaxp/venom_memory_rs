//! Python Templates for VenomMemory projects
//!
//! Generates a complete Python project with:
//! - C daemon (for system monitoring)
//! - Python client with ctypes FFI bindings
//! - Bundled libvenom_memory.so

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    // Create directories
    crate::create_dir(&format!("{}/lib", base));
    crate::create_dir(&format!("{}/daemon/src", base));
    crate::create_dir(&format!("{}/shared", base));
    
    // Shared protocol (C header)
    crate::write_file(&format!("{}/shared/protocol.h", base), &protocol_h(config));
    
    // C Daemon
    crate::write_file(&format!("{}/daemon/src/main.c", base), &daemon_main(config));
    crate::write_file(&format!("{}/daemon/Makefile", base), &daemon_makefile(config));
    
    // Python client
    crate::write_file(&format!("{}/venom_binding.py", base), &venom_binding(config));
    crate::write_file(&format!("{}/client.py", base), &client_py(config));
    
    // README
    crate::write_file(&format!("{}/README.md", base), &readme(config));
}

fn magic(channel: &str) -> u32 {
    channel.bytes().fold(0x564E4Fu32, |acc, b| acc.wrapping_add(b as u32))
}

fn upper_name(name: &str) -> String {
    name.to_uppercase().replace("-", "_")
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

// ═══════════════════════════════════════════════════════════════════════════
// C Protocol Header (shared between daemon and Python client)
// ═══════════════════════════════════════════════════════════════════════════

fn protocol_h(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"#ifndef {upper}_PROTOCOL_H
#define {upper}_PROTOCOL_H

#include <stdint.h>
#include <stdbool.h>

#define {upper}_CHANNEL_NAME "{channel}"
#define {upper}_MAGIC 0x{magic:08X}
#define {upper}_DATA_SIZE {data_size}
#define {upper}_CMD_SLOTS {cmd_slots}
#define {upper}_MAX_CLIENTS {max_clients}
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

#endif // {upper}_PROTOCOL_H
"#,
        upper = upper,
        pascal = pascal,
        channel = config.channel,
        magic = magic(&config.channel),
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// C Daemon
// ═══════════════════════════════════════════════════════════════════════════

fn daemon_main(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"/**
 * {name} System Monitor Daemon
 * Reads CPU/RAM/Uptime from /proc and publishes to shared memory.
 * Python client connects to this daemon.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
#include <time.h>
#include "../shared/protocol.h"

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

static void read_cpu_stats(void) {{
    FILE* f = fopen("/proc/stat", "r");
    if (!f) return;
    char line[256];
    int core_idx = 0;
    while (fgets(line, sizeof(line), f) && core_idx <= {upper}_MAX_CORES) {{
        if (strncmp(line, "cpu", 3) != 0) continue;
        uint64_t user, nice, system, idle, iowait, irq, softirq;
        if (sscanf(line + (line[3] == ' ' ? 4 : 5), "%lu %lu %lu %lu %lu %lu %lu",
                   &user, &nice, &system, &idle, &iowait, &irq, &softirq) != 7) continue;
        uint64_t total = user + nice + system + idle + iowait + irq + softirq;
        uint64_t idle_time = idle + iowait;
        uint64_t total_delta = total - prev_total[core_idx];
        uint64_t idle_delta = idle_time - prev_idle[core_idx];
        float usage = total_delta > 0 ? (1.0f - (float)idle_delta / (float)total_delta) * 100.0f : 0;
        if (line[3] == ' ') g_state.cpu_usage_percent = usage;
        else if (core_idx - 1 >= 0 && core_idx - 1 < {upper}_MAX_CORES) g_state.cpu_cores[core_idx - 1] = usage;
        prev_total[core_idx] = total;
        prev_idle[core_idx] = idle_time;
        core_idx++;
    }}
    g_state.core_count = core_idx > 1 ? core_idx - 1 : 0;
    fclose(f);
}}

static void read_memory_stats(void) {{
    FILE* f = fopen("/proc/meminfo", "r");
    if (!f) return;
    char line[256];
    uint64_t total_kb = 0, available_kb = 0;
    while (fgets(line, sizeof(line), f)) {{
        if (strncmp(line, "MemTotal:", 9) == 0) sscanf(line + 9, "%lu", &total_kb);
        else if (strncmp(line, "MemAvailable:", 13) == 0) sscanf(line + 13, "%lu", &available_kb);
    }}
    g_state.memory_total_mb = (uint32_t)(total_kb / 1024);
    g_state.memory_used_mb = (uint32_t)((total_kb - available_kb) / 1024);
    fclose(f);
}}

static void read_uptime(void) {{
    FILE* f = fopen("/proc/uptime", "r");
    if (!f) return;
    double uptime;
    if (fscanf(f, "%lf", &uptime) == 1) g_state.uptime_seconds = (uint64_t)uptime;
    fclose(f);
}}

int main(void) {{
    printf("🖥️  {name} System Monitor Daemon\n");
    printf("═══════════════════════════════════════════════════════════════\n");
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);
    
    VenomConfig config = {{ .data_size = {upper}_DATA_SIZE, .cmd_slots = {upper}_CMD_SLOTS, .max_clients = {upper}_MAX_CLIENTS }};
    g_daemon = venom_daemon_create({upper}_CHANNEL_NAME, config);
    if (!g_daemon) {{ printf("❌ Failed to create channel\n"); return 1; }}
    
    g_state.magic = {upper}_MAGIC;
    g_state.version = 1;
    
    printf("✅ Channel: %s\n", {upper}_CHANNEL_NAME);
    printf("🐍 Python client can connect now!\n");
    printf("🚀 Publishing... (Ctrl+C to stop)\n\n");
    
    while (g_running) {{
        read_cpu_stats();
        read_memory_stats();
        read_uptime();
        g_state.update_counter++;
        struct timespec ts;
        clock_gettime(CLOCK_MONOTONIC, &ts);
        g_state.timestamp_ns = (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
        venom_daemon_write_data(g_daemon, (const uint8_t*)&g_state, sizeof(g_state));
        
        printf("\r🖥️  CPU: %5.1f%% | RAM: %u/%u MB | Uptime: %luh%lum | #%lu   ",
            g_state.cpu_usage_percent, g_state.memory_used_mb, g_state.memory_total_mb,
            (unsigned long)(g_state.uptime_seconds / 3600), (unsigned long)((g_state.uptime_seconds % 3600) / 60),
            (unsigned long)g_state.update_counter);
        fflush(stdout);
        usleep(100000);
    }}
    venom_daemon_destroy(g_daemon);
    printf("\n\n👋 Goodbye!\n");
    return 0;
}}
"#, name = config.name, upper = upper, pascal = pascal)
}

fn daemon_makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Daemon Makefile

CC = gcc
CFLAGS = -Wall -Wextra -O2 -I../shared
LDFLAGS = -L../lib -lvenom_memory -Wl,-rpath,'$$ORIGIN/../lib'

TARGET = {name}_daemon
SOURCES = src/main.c

.PHONY: all clean run

all: $(TARGET)

$(TARGET): $(SOURCES)
	@echo "🔗 Building $(TARGET)..."
	@$(CC) $(CFLAGS) $(SOURCES) -o $(TARGET) $(LDFLAGS)
	@echo "✅ Build complete"

clean:
	@rm -f $(TARGET)

run: $(TARGET)
	@./$(TARGET)
"#, name = config.name)
}

// ═══════════════════════════════════════════════════════════════════════════
// Python Bindings
// ═══════════════════════════════════════════════════════════════════════════

fn venom_binding(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r#"#!/usr/bin/env python3
"""
VenomMemory Python Bindings for {name}

Provides:
- {pascal}State: System stats from daemon
- VenomShell: Connection to daemon via shared memory
"""

import ctypes
import struct
from dataclasses import dataclass
from typing import List, Optional
from pathlib import Path

# ═══════════════════════════════════════════════════════════════════════════
# Configuration
# ═══════════════════════════════════════════════════════════════════════════

CHANNEL_NAME = "{channel}"
MAGIC = 0x{magic:08X}
MAX_CORES = 16

# ═══════════════════════════════════════════════════════════════════════════
# State Structure
# ═══════════════════════════════════════════════════════════════════════════

@dataclass
class {pascal}State:
    """System state published by the daemon."""
    magic: int
    version: int
    cpu_usage_percent: float
    cpu_cores: List[float]
    core_count: int
    memory_used_mb: int
    memory_total_mb: int
    uptime_seconds: int
    update_counter: int
    timestamp_ns: int
    
    @property
    def is_valid(self) -> bool:
        return self.magic == MAGIC
    
    @property
    def memory_usage_percent(self) -> float:
        if self.memory_total_mb > 0:
            return self.memory_used_mb / self.memory_total_mb * 100
        return 0.0
    
    @property
    def uptime_formatted(self) -> str:
        hours = self.uptime_seconds // 3600
        minutes = (self.uptime_seconds % 3600) // 60
        return f"{{hours}}h {{minutes}}m"
    
    @classmethod
    def from_bytes(cls, data: bytes) -> '{pascal}State':
        if len(data) < 112:
            return cls.empty()
        magic, version, cpu_usage = struct.unpack_from('<IIf', data, 0)
        cpu_cores = list(struct.unpack_from('<16f', data, 12))
        core_count, mem_used, mem_total = struct.unpack_from('<III', data, 76)
        uptime, counter, timestamp = struct.unpack_from('<QQQ', data, 88)
        return cls(magic=magic, version=version, cpu_usage_percent=cpu_usage,
                   cpu_cores=cpu_cores, core_count=core_count,
                   memory_used_mb=mem_used, memory_total_mb=mem_total,
                   uptime_seconds=uptime, update_counter=counter, timestamp_ns=timestamp)
    
    @classmethod
    def empty(cls) -> '{pascal}State':
        return cls(magic=0, version=0, cpu_usage_percent=0.0,
                   cpu_cores=[0.0] * MAX_CORES, core_count=0,
                   memory_used_mb=0, memory_total_mb=0,
                   uptime_seconds=0, update_counter=0, timestamp_ns=0)

# ═══════════════════════════════════════════════════════════════════════════
# VenomShell - Connection to Daemon
# ═══════════════════════════════════════════════════════════════════════════

def _find_library() -> str:
    script_dir = Path(__file__).parent.absolute()
    locations = [
        script_dir / "lib" / "libvenom_memory.so",
        script_dir / "../lib" / "libvenom_memory.so",
        Path("lib/libvenom_memory.so"),
    ]
    for path in locations:
        if path.exists():
            return str(path.absolute())
    raise FileNotFoundError(f"libvenom_memory.so not found in: {{locations}}")


class VenomShell:
    """Connection to VenomMemory daemon."""
    
    _lib: Optional[ctypes.CDLL] = None
    
    def __init__(self, channel_name: str = CHANNEL_NAME):
        self._handle = None
        self._disposed = False  # Initialize BEFORE connection attempt
        
        if VenomShell._lib is None:
            VenomShell._lib = ctypes.CDLL(_find_library())
            self._setup_bindings()
        
        channel_bytes = channel_name.encode('utf-8')
        self._handle = VenomShell._lib.venom_shell_connect(channel_bytes)
        
        if not self._handle:
            raise ConnectionError(f"Failed to connect to '{{channel_name}}'. Is daemon running?")
    
    def _setup_bindings(self):
        lib = VenomShell._lib
        lib.venom_shell_connect.argtypes = [ctypes.c_char_p]
        lib.venom_shell_connect.restype = ctypes.c_void_p
        lib.venom_shell_destroy.argtypes = [ctypes.c_void_p]
        lib.venom_shell_destroy.restype = None
        lib.venom_shell_read_data.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]
        lib.venom_shell_read_data.restype = ctypes.c_size_t
        lib.venom_shell_id.argtypes = [ctypes.c_void_p]
        lib.venom_shell_id.restype = ctypes.c_uint32
    
    @property
    def client_id(self) -> int:
        self._check_disposed()
        return VenomShell._lib.venom_shell_id(self._handle)
    
    def read_raw_data(self, max_len: int = 256) -> bytes:
        self._check_disposed()
        buf = (ctypes.c_uint8 * max_len)()
        length = VenomShell._lib.venom_shell_read_data(self._handle, buf, max_len)
        return bytes(buf[:length])
    
    def read_state(self) -> {pascal}State:
        return {pascal}State.from_bytes(self.read_raw_data(256))
    
    def close(self):
        if self._disposed or not self._handle:
            return
        self._disposed = True
        VenomShell._lib.venom_shell_destroy(self._handle)
        self._handle = None
    
    def _check_disposed(self):
        if self._disposed:
            raise RuntimeError("VenomShell has been closed")
    
    def __enter__(self): return self
    def __exit__(self, *_): self.close()
    def __del__(self): self.close()


if __name__ == "__main__":
    print(f"Channel: {{CHANNEL_NAME}} | Magic: 0x{{MAGIC:08X}}")
    try:
        with VenomShell() as shell:
            print(f"Connected! ID: {{shell.client_id}}")
            state = shell.read_state()
            print(f"CPU: {{state.cpu_usage_percent:.1f}}% | RAM: {{state.memory_used_mb}}/{{state.memory_total_mb}} MB")
    except Exception as e:
        print(f"Error: {{e}}")
"#,
        name = config.name,
        channel = config.channel,
        magic = magic(&config.channel),
        pascal = pascal
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Python Client
// ═══════════════════════════════════════════════════════════════════════════

fn client_py(config: &ProjectConfig) -> String {
    
    format!(r#"#!/usr/bin/env python3
"""
{name} Status Bar - VenomMemory Python Client
Displays live CPU/RAM/Uptime stats with colored progress bars.
Includes read latency benchmarking.
"""

import sys
import time
from venom_binding import VenomShell, CHANNEL_NAME

# ANSI colors
G, Y, R, C, RST = '\033[92m', '\033[93m', '\033[91m', '\033[96m', '\033[0m'

# Latency tracking
latency_min = float('inf')
latency_max = 0.0
latency_sum = 0.0
latency_count = 0

def bar(pct: float, w: int = 25) -> str:
    filled = int((pct / 100) * w)
    c = R if pct > 80 else Y if pct > 50 else G
    return "[" + "".join(c + "█" + RST if i < filled else " " for i in range(w)) + "]"

def main():
    global latency_min, latency_max, latency_sum, latency_count
    
    print("╔═══════════════════════════════════════════════════════════════╗")
    print("║   🖥️  {name} Status Bar (Python)                              ║")
    print("╚═══════════════════════════════════════════════════════════════╝\n")
    
    try:
        shell = VenomShell()
        print(f"✅ Connected! ID: {{shell.client_id}}")
        print("📊 Reading stats... (Ctrl+C to exit)\n")
        time.sleep(1)
        
        frame = 0
        while True:
            # ═══════════════════════════════════════════════════════════════════
            # 📊 BENCHMARK: Measure read latency
            # ═══════════════════════════════════════════════════════════════════
            t_start = time.perf_counter_ns()
            state = shell.read_state()
            t_end = time.perf_counter_ns()
            latency_us = (t_end - t_start) / 1000.0
            
            # Update stats
            if latency_us < latency_min: latency_min = latency_us
            if latency_us > latency_max: latency_max = latency_us
            latency_sum += latency_us
            latency_count += 1
            avg_us = latency_sum / latency_count
            
            if state.is_valid:
                print('\033[2J\033[H', end='')  # Clear screen
                print("╔═══════════════════════════════════════════════════════════════╗")
                print(f"║  🖥️  {name} Monitor (Python)  Frame: {{frame:<6}}                   ║")
                print("╠═══════════════════════════════════════════════════════════════╣")
                print(f"║  CPU: {{bar(state.cpu_usage_percent)}} {{state.cpu_usage_percent:5.1f}}%             ║")
                print("╠═══════════════════════════════════════════════════════════════╣")
                for i in range(state.core_count):
                    print(f"║  Core {{i}}: {{bar(state.cpu_cores[i], 20)}} {{state.cpu_cores[i]:5.1f}}%                ║")
                print("╠═══════════════════════════════════════════════════════════════╣")
                print(f"║  RAM: {{bar(state.memory_usage_percent)}} {{state.memory_used_mb}}/{{state.memory_total_mb}} MB      ║")
                print("╠═══════════════════════════════════════════════════════════════╣")
                print(f"║  ⏱️ Uptime: {{state.uptime_formatted}}                                        ║")
                print("╠═══════════════════════════════════════════════════════════════╣")
                print(f"║  📊 {{C}}Read Latency:{{RST}} {{latency_us:.2f}} µs (min: {{latency_min:.2f}}, max: {{latency_max:.2f}}, avg: {{avg_us:.2f}})  ║")
                print("╚═══════════════════════════════════════════════════════════════╝")
                print(f"  Cores: {{state.core_count}} | Updates: {{state.update_counter}} | Ctrl+C to exit")
                frame += 1
            time.sleep(0.1)
    except KeyboardInterrupt:
        print("\n")
        print(f"📊 {{C}}Final Latency Stats (Python):{{RST}}")
        print(f"   Samples: {{latency_count}}")
        print(f"   Min: {{latency_min:.2f}} µs")
        print(f"   Max: {{latency_max:.2f}} µs")
        print(f"   Avg: {{latency_sum / latency_count:.2f}} µs")
        print("\n👋 Goodbye!")
    except Exception as e:
        print(f"\n❌ Error: {{e}}")
        print("\nMake sure daemon is running: cd daemon && make run")
        sys.exit(1)

if __name__ == "__main__":
    main()
"#, name = config.name)
}

// ═══════════════════════════════════════════════════════════════════════════
// README
// ═══════════════════════════════════════════════════════════════════════════

fn readme(config: &ProjectConfig) -> String {
    format!(r#"# {name} (Python + C Daemon)

VenomMemory project with C daemon and Python client.

## Quick Start

```bash
# Terminal 1 - Start C daemon
cd daemon && make run

# Terminal 2 - Start Python client
python3 client.py
```

## Structure

```
{name}/
├── daemon/           # C daemon (system monitor)
│   ├── src/main.c
│   └── Makefile
├── shared/           # Shared protocol
│   └── protocol.h
├── venom_binding.py  # Python FFI bindings
├── client.py         # Python status bar
└── lib/
    └── libvenom_memory.so
```

## Configuration

| Setting | Value |
|---------|-------|
| Channel | `{channel}` |
| Magic | `0x{magic:08X}` |
"#,
        name = config.name,
        channel = config.channel,
        magic = magic(&config.channel)
    )
}
