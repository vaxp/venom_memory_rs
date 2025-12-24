//! C++ Templates for VenomMemory projects
//!
//! Generates a complete C++ project with:
//! - Modern C++ wrapper classes (RAII)
//! - System monitor daemon
//! - Status bar client

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    crate::create_dir(&format!("{}/shared", base));
    crate::create_dir(&format!("{}/daemon/src", base));
    crate::create_dir(&format!("{}/client/src", base));
    
    // Shared
    crate::write_file(&format!("{}/shared/protocol.hpp", base), &protocol_hpp(config));
    crate::write_file(&format!("{}/shared/venom.hpp", base), &venom_hpp(config));
    
    // Daemon
    crate::write_file(&format!("{}/daemon/src/main.cpp", base), &daemon_main(config));
    crate::write_file(&format!("{}/daemon/Makefile", base), &daemon_makefile(config));
    
    // Client
    crate::write_file(&format!("{}/client/src/main.cpp", base), &client_main(config));
    crate::write_file(&format!("{}/client/Makefile", base), &client_makefile(config));
    
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
// Protocol Header (C++ style)
// ═══════════════════════════════════════════════════════════════════════════

fn protocol_hpp(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"#pragma once
#include <cstdint>
#include <string>
#include <array>

namespace {ns} {{

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

constexpr const char* CHANNEL_NAME = "{channel}";
constexpr uint32_t MAGIC = 0x{magic:08X};
constexpr size_t DATA_SIZE = {data_size};
constexpr size_t CMD_SLOTS = {cmd_slots};
constexpr size_t MAX_CLIENTS = {max_clients};
constexpr size_t MAX_CORES = 16;

// ═══════════════════════════════════════════════════════════════════════════
// State Structure
// ═══════════════════════════════════════════════════════════════════════════

#pragma pack(push, 1)
struct State {{
    uint32_t magic = 0;
    uint32_t version = 0;
    float cpu_usage_percent = 0.0f;
    std::array<float, MAX_CORES> cpu_cores{{}};
    uint32_t core_count = 0;
    uint32_t memory_used_mb = 0;
    uint32_t memory_total_mb = 0;
    uint64_t uptime_seconds = 0;
    uint64_t update_counter = 0;
    uint64_t timestamp_ns = 0;
    
    [[nodiscard]] bool is_valid() const {{ return magic == MAGIC; }}
    
    [[nodiscard]] float memory_percent() const {{
        return memory_total_mb > 0 ? 
            static_cast<float>(memory_used_mb) / memory_total_mb * 100.0f : 0.0f;
    }}
    
    [[nodiscard]] std::string uptime_formatted() const {{
        auto h = uptime_seconds / 3600;
        auto m = (uptime_seconds % 3600) / 60;
        return std::to_string(h) + "h " + std::to_string(m) + "m";
    }}
}};
#pragma pack(pop)

static_assert(sizeof(State) == 112, "State struct size mismatch");

}} // namespace {ns}
"#,
        ns = pascal.to_lowercase(),
        channel = config.channel,
        magic = magic(&config.channel),
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// C++ Wrapper for VenomMemory
// ═══════════════════════════════════════════════════════════════════════════

fn venom_hpp(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r#"#pragma once
#include "protocol.hpp"
#include <memory>
#include <stdexcept>
#include <cstring>

// C bindings
extern "C" {{
    struct VenomConfig {{ size_t data_size; size_t cmd_slots; size_t max_clients; }};
    void* venom_daemon_create(const char* name, VenomConfig config);
    void venom_daemon_destroy(void* handle);
    void venom_daemon_write_data(void* handle, const uint8_t* data, size_t len);
    size_t venom_daemon_try_recv_command(void* handle, uint8_t* buf, size_t max_len, uint32_t* out_client_id);
    
    void* venom_shell_connect(const char* name);
    void venom_shell_destroy(void* handle);
    size_t venom_shell_read_data(void* handle, uint8_t* buf, size_t max_len);
    uint32_t venom_shell_id(void* handle);
}}

namespace {ns} {{

// ═══════════════════════════════════════════════════════════════════════════
// RAII Daemon Wrapper
// ═══════════════════════════════════════════════════════════════════════════

class Daemon {{
public:
    Daemon() {{
        VenomConfig cfg{{DATA_SIZE, CMD_SLOTS, MAX_CLIENTS}};
        handle_ = venom_daemon_create(CHANNEL_NAME, cfg);
        if (!handle_) throw std::runtime_error("Failed to create daemon channel");
    }}
    
    ~Daemon() {{ if (handle_) venom_daemon_destroy(handle_); }}
    
    // Non-copyable, movable
    Daemon(const Daemon&) = delete;
    Daemon& operator=(const Daemon&) = delete;
    Daemon(Daemon&& other) noexcept : handle_(other.handle_) {{ other.handle_ = nullptr; }}
    
    void write(const State& state) {{
        venom_daemon_write_data(handle_, reinterpret_cast<const uint8_t*>(&state), sizeof(State));
    }}
    
    [[nodiscard]] bool try_recv_command(uint8_t* buf, size_t max_len, uint32_t& client_id) {{
        return venom_daemon_try_recv_command(handle_, buf, max_len, &client_id) > 0;
    }}

private:
    void* handle_ = nullptr;
}};

// ═══════════════════════════════════════════════════════════════════════════
// RAII Shell (Client) Wrapper
// ═══════════════════════════════════════════════════════════════════════════

class Shell {{
public:
    Shell() {{
        handle_ = venom_shell_connect(CHANNEL_NAME);
        if (!handle_) throw std::runtime_error("Failed to connect - is daemon running?");
    }}
    
    ~Shell() {{ if (handle_) venom_shell_destroy(handle_); }}
    
    // Non-copyable, movable
    Shell(const Shell&) = delete;
    Shell& operator=(const Shell&) = delete;
    Shell(Shell&& other) noexcept : handle_(other.handle_) {{ other.handle_ = nullptr; }}
    
    [[nodiscard]] uint32_t client_id() const {{ return venom_shell_id(handle_); }}
    
    [[nodiscard]] State read_state() {{
        State state{{}};
        uint8_t buf[256];
        size_t len = venom_shell_read_data(handle_, buf, sizeof(buf));
        if (len >= sizeof(State)) std::memcpy(&state, buf, sizeof(State));
        return state;
    }}

private:
    void* handle_ = nullptr;
}};

}} // namespace {ns}
"#, ns = pascal.to_lowercase())
}

// ═══════════════════════════════════════════════════════════════════════════
// Daemon
// ═══════════════════════════════════════════════════════════════════════════

fn daemon_main(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    let ns = pascal.to_lowercase();
    
    format!(r##"/**
 * {name} System Monitor Daemon (C++)
 */

#include "../shared/venom.hpp"
#include <iostream>
#include <fstream>
#include <sstream>
#include <chrono>
#include <thread>
#include <csignal>
#include <iomanip>
#include <vector>

using namespace {ns};

static volatile bool g_running = true;
static std::vector<uint64_t> prev_total(MAX_CORES + 1, 0);
static std::vector<uint64_t> prev_idle(MAX_CORES + 1, 0);

void signal_handler(int) {{ g_running = false; }}

void read_cpu(State& state) {{
    std::ifstream f("/proc/stat");
    if (!f) return;
    std::string line;
    size_t core_idx = 0;
    while (std::getline(f, line) && core_idx <= MAX_CORES) {{
        if (line.substr(0, 3) != "cpu") continue;
        std::istringstream iss(line);
        std::string cpu;
        uint64_t user, nice, system, idle, iowait, irq, softirq;
        iss >> cpu >> user >> nice >> system >> idle >> iowait >> irq >> softirq;
        uint64_t total = user + nice + system + idle + iowait + irq + softirq;
        uint64_t idle_time = idle + iowait;
        uint64_t total_d = total - prev_total[core_idx];
        uint64_t idle_d = idle_time - prev_idle[core_idx];
        float usage = total_d > 0 ? (1.0f - static_cast<float>(idle_d) / total_d) * 100.0f : 0;
        if (cpu == "cpu") state.cpu_usage_percent = usage;
        else if (core_idx > 0 && core_idx <= MAX_CORES) state.cpu_cores[core_idx - 1] = usage;
        prev_total[core_idx] = total;
        prev_idle[core_idx] = idle_time;
        core_idx++;
    }}
    state.core_count = core_idx > 1 ? core_idx - 1 : 0;
}}

void read_memory(State& state) {{
    std::ifstream f("/proc/meminfo");
    if (!f) return;
    std::string line;
    uint64_t total_kb = 0, avail_kb = 0;
    while (std::getline(f, line)) {{
        if (line.substr(0, 9) == "MemTotal:") std::sscanf(line.c_str() + 9, "%lu", &total_kb);
        if (line.substr(0, 13) == "MemAvailable:") std::sscanf(line.c_str() + 13, "%lu", &avail_kb);
    }}
    state.memory_total_mb = total_kb / 1024;
    state.memory_used_mb = (total_kb - avail_kb) / 1024;
}}

void read_uptime(State& state) {{
    std::ifstream f("/proc/uptime");
    double uptime;
    if (f >> uptime) state.uptime_seconds = static_cast<uint64_t>(uptime);
}}

int main() {{
    std::cout << "🖥️  {name} System Monitor (C++)\n";
    std::cout << "═══════════════════════════════════════════════════════════════\n";
    
    std::signal(SIGINT, signal_handler);
    std::signal(SIGTERM, signal_handler);
    
    try {{
        Daemon daemon;
        std::cout << "✅ Channel: " << CHANNEL_NAME << "\n";
        std::cout << "🚀 Publishing... (Ctrl+C to stop)\n\n";
        
        State state{{}};
        state.magic = MAGIC;
        state.version = 1;
        
        while (g_running) {{
            read_cpu(state);
            read_memory(state);
            read_uptime(state);
            state.update_counter++;
            auto now = std::chrono::steady_clock::now().time_since_epoch();
            state.timestamp_ns = std::chrono::duration_cast<std::chrono::nanoseconds>(now).count();
            
            daemon.write(state);
            
            std::cout << "\r🖥️  CPU: " << std::fixed << std::setprecision(1) << state.cpu_usage_percent
                      << "% | RAM: " << state.memory_used_mb << "/" << state.memory_total_mb << " MB"
                      << " | #" << state.update_counter << "   " << std::flush;
            
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }}
        
        std::cout << "\n\n👋 Goodbye!\n";
    }} catch (const std::exception& e) {{
        std::cerr << "❌ Error: " << e.what() << "\n";
        return 1;
    }}
    return 0;
}}
"##, name = config.name, ns = ns)
}

fn daemon_makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Daemon Makefile (C++)

CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2 -I../shared
LDFLAGS = -L../lib -lvenom_memory -Wl,-rpath,'$$ORIGIN/../lib'

TARGET = {name}_daemon
SOURCES = src/main.cpp

.PHONY: all clean run

all: $(TARGET)

$(TARGET): $(SOURCES)
	@echo "🔗 Building $(TARGET)..."
	@$(CXX) $(CXXFLAGS) $(SOURCES) -o $(TARGET) $(LDFLAGS)
	@echo "✅ Build complete"

clean:
	@rm -f $(TARGET)

run: $(TARGET)
	@./$(TARGET)
"#, name = config.name)
}

// ═══════════════════════════════════════════════════════════════════════════
// Client
// ═══════════════════════════════════════════════════════════════════════════

fn client_main(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    let ns = pascal.to_lowercase();
    
    format!(r##"/**
 * {name} Status Bar Client (C++) - with Benchmarking
 */

#include "../shared/venom.hpp"
#include <iostream>
#include <iomanip>
#include <thread>
#include <csignal>
#include <chrono>
#include <limits>

using namespace {ns};

static volatile bool g_running = true;

// Latency tracking
static double g_latency_min = std::numeric_limits<double>::max();
static double g_latency_max = 0.0;
static double g_latency_sum = 0.0;
static uint64_t g_latency_count = 0;

void signal_handler(int) {{ g_running = false; }}

// ANSI colors
const char* G = "\033[92m";
const char* Y = "\033[93m";
const char* R = "\033[91m";
const char* C = "\033[96m";
const char* RST = "\033[0m";

void print_bar(float pct, int width = 25) {{
    int filled = static_cast<int>((pct / 100.0f) * width);
    const char* color = pct > 80 ? R : pct > 50 ? Y : G;
    std::cout << "[";
    for (int i = 0; i < width; i++) {{
        if (i < filled) std::cout << color << "█" << RST;
        else std::cout << " ";
    }}
    std::cout << "]";
}}

int main() {{
    std::cout << "╔═══════════════════════════════════════════════════════════════╗\n";
    std::cout << "║   🖥️  {name} Status Bar (C++)                                  ║\n";
    std::cout << "╚═══════════════════════════════════════════════════════════════╝\n\n";
    
    std::signal(SIGINT, signal_handler);
    std::signal(SIGTERM, signal_handler);
    
    try {{
        Shell shell;
        std::cout << "✅ Connected! ID: " << shell.client_id() << "\n";
        std::cout << "📊 Reading stats... (Ctrl+C to exit)\n\n";
        std::this_thread::sleep_for(std::chrono::seconds(1));
        
        int frame = 0;
        while (g_running) {{
            // ═══════════════════════════════════════════════════════════════════
            // 📊 BENCHMARK: Measure read latency
            // ═══════════════════════════════════════════════════════════════════
            auto t_start = std::chrono::high_resolution_clock::now();
            auto state = shell.read_state();
            auto t_end = std::chrono::high_resolution_clock::now();
            double latency_us = std::chrono::duration<double, std::micro>(t_end - t_start).count();
            
            // Update stats
            if (latency_us < g_latency_min) g_latency_min = latency_us;
            if (latency_us > g_latency_max) g_latency_max = latency_us;
            g_latency_sum += latency_us;
            g_latency_count++;
            double avg_us = g_latency_sum / g_latency_count;
            
            if (state.is_valid()) {{
                std::cout << "\033[2J\033[H";  // Clear screen
                std::cout << "╔═══════════════════════════════════════════════════════════════╗\n";
                std::cout << "║  🖥️  {name} Monitor (C++)        Frame: " << std::setw(6) << std::left << frame++ << "             ║\n";
                std::cout << "╠═══════════════════════════════════════════════════════════════╣\n";
                std::cout << "║  CPU: "; print_bar(state.cpu_usage_percent);
                std::cout << " " << std::fixed << std::setprecision(1) << std::setw(5) << state.cpu_usage_percent << "%             ║\n";
                std::cout << "╠═══════════════════════════════════════════════════════════════╣\n";
                
                for (uint32_t i = 0; i < state.core_count; i++) {{
                    std::cout << "║  Core " << i << ": "; print_bar(state.cpu_cores[i], 20);
                    std::cout << " " << std::fixed << std::setprecision(1) << std::setw(5) << state.cpu_cores[i] << "%                ║\n";
                }}
                
                std::cout << "╠═══════════════════════════════════════════════════════════════╣\n";
                std::cout << "║  RAM: "; print_bar(state.memory_percent());
                std::cout << " " << state.memory_used_mb << "/" << state.memory_total_mb << " MB      ║\n";
                std::cout << "╠═══════════════════════════════════════════════════════════════╣\n";
                std::cout << "║  ⏱️ Uptime: " << state.uptime_formatted() << "                                        ║\n";
                std::cout << "╠═══════════════════════════════════════════════════════════════╣\n";
                std::cout << "║  📊 " << C << "Read Latency:" << RST << " " << std::fixed << std::setprecision(2) 
                          << latency_us << " µs (min: " << g_latency_min << ", max: " << g_latency_max << ", avg: " << avg_us << ")  ║\n";
                std::cout << "╚═══════════════════════════════════════════════════════════════╝\n";
                std::cout << "  Cores: " << state.core_count << " | Updates: " << state.update_counter << " | Ctrl+C to exit\n";
            }}
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }}
        
        // Print final stats
        std::cout << "\n\n📊 " << C << "Final Latency Stats (C++):" << RST << "\n";
        std::cout << "   Samples: " << g_latency_count << "\n";
        std::cout << "   Min: " << std::fixed << std::setprecision(2) << g_latency_min << " µs\n";
        std::cout << "   Max: " << g_latency_max << " µs\n";
        std::cout << "   Avg: " << (g_latency_sum / g_latency_count) << " µs\n";
        
        std::cout << "\n👋 Goodbye!\n";
    }} catch (const std::exception& e) {{
        std::cerr << "❌ Error: " << e.what() << "\n";
        return 1;
    }}
    return 0;
}}
"##, name = config.name, ns = ns)
}

fn client_makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Client Makefile (C++)

CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2 -I../shared
LDFLAGS = -L../lib -lvenom_memory -Wl,-rpath,'$$ORIGIN/../lib'

TARGET = {name}_client
SOURCES = src/main.cpp

.PHONY: all clean run

all: $(TARGET)

$(TARGET): $(SOURCES)
	@echo "🔗 Building $(TARGET)..."
	@$(CXX) $(CXXFLAGS) $(SOURCES) -o $(TARGET) $(LDFLAGS)
	@echo "✅ Build complete"

clean:
	@rm -f $(TARGET)

run: $(TARGET)
	@./$(TARGET)
"#, name = config.name)
}

fn readme(config: &ProjectConfig) -> String {
    format!(r#"# {name} (C++)

VenomMemory C++ system monitor with RAII wrappers.

## Quick Start

```bash
# Terminal 1 - Daemon
cd daemon && make run

# Terminal 2 - Client
cd client && make run
```

## Features

- Modern C++17
- RAII wrappers (automatic cleanup)
- Move semantics support
- Type-safe State struct

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
