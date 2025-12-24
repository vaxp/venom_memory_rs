//! C Templates for VenomMemory projects

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    crate::create_dir(&format!("{}/shared", base));
    crate::create_dir(&format!("{}/daemon/src", base));
    crate::create_dir(&format!("{}/client/src", base));
    
    // Protocol header
    crate::write_file(&format!("{}/shared/protocol.h", base), &protocol_h(config));
    
    // Daemon
    crate::write_file(&format!("{}/daemon/src/main.c", base), &daemon_main(config));
    crate::write_file(&format!("{}/daemon/Makefile", base), &daemon_makefile(config));
    
    // Client
    crate::write_file(&format!("{}/client/src/main.c", base), &client_main(config));
    crate::write_file(&format!("{}/client/Makefile", base), &client_makefile(config));
    
    // README
    crate::write_file(&format!("{}/README.md", base), &readme(config));
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

fn magic(channel: &str) -> u32 {
    channel.bytes().fold(0x564E4Fu32, |acc, b| acc.wrapping_add(b as u32))
}

// ═══════════════════════════════════════════════════════════════════════════
// Protocol Header
// ═══════════════════════════════════════════════════════════════════════════

fn protocol_h(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"#ifndef {upper}_PROTOCOL_H
#define {upper}_PROTOCOL_H

#include <stdint.h>
#include <stdbool.h>

// ═══════════════════════════════════════════════════════════════════════════
// 📡 Channel Configuration
// ═══════════════════════════════════════════════════════════════════════════

#define {upper}_CHANNEL_NAME "{channel}"
#define {upper}_MAGIC 0x{magic:08X}
#define {upper}_DATA_SIZE {data_size}
#define {upper}_CMD_SLOTS {cmd_slots}
#define {upper}_MAX_CLIENTS {max_clients}
#define {upper}_MAX_CORES 16

// ═══════════════════════════════════════════════════════════════════════════
// 📊 System Stats (Daemon writes, Clients read)
// ═══════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════
// 📨 Commands
// ═══════════════════════════════════════════════════════════════════════════

typedef enum {{
    CMD_REFRESH = 1,
    CMD_SET_INTERVAL,
}} {pascal}CmdType;

typedef struct __attribute__((packed)) {{
    uint8_t cmd;
    uint8_t _pad[3];
    int32_t value;
}} {pascal}Command;

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
// Daemon
// ═══════════════════════════════════════════════════════════════════════════

fn daemon_main(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"/**
 * {name} System Monitor Daemon - VenomMemory IPC
 * Reads CPU/RAM/Uptime from /proc and publishes to shared memory.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
#include <time.h>
#include "../shared/protocol.h"

// VenomMemory bindings
typedef struct VenomDaemonHandle VenomDaemonHandle;
typedef struct {{ size_t data_size; size_t cmd_slots; size_t max_clients; }} VenomConfig;
extern VenomDaemonHandle* venom_daemon_create(const char* name, VenomConfig config);
extern void venom_daemon_destroy(VenomDaemonHandle* handle);
extern void venom_daemon_write_data(VenomDaemonHandle* handle, const uint8_t* data, size_t len);
extern size_t venom_daemon_try_recv_command(VenomDaemonHandle* handle, uint8_t* buf, size_t max_len, uint32_t* out_client_id);

static VenomDaemonHandle* g_daemon = NULL;
static {pascal}State g_state = {{0}};
static volatile int g_running = 1;
static uint64_t g_counter = 0;
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

static void update_stats(void) {{
    read_cpu_stats();
    read_memory_stats();
    read_uptime();
    g_state.magic = {upper}_MAGIC;
    g_state.version = 1;
    g_state.update_counter = ++g_counter;
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    g_state.timestamp_ns = (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
    venom_daemon_write_data(g_daemon, (const uint8_t*)&g_state, sizeof(g_state));
}}

int main(void) {{
    printf("🖥️  {name} System Monitor (VenomMemory)\n");
    printf("═══════════════════════════════════════════════════════════════\n");
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);
    
    VenomConfig config = {{ .data_size = {upper}_DATA_SIZE, .cmd_slots = {upper}_CMD_SLOTS, .max_clients = {upper}_MAX_CLIENTS }};
    g_daemon = venom_daemon_create({upper}_CHANNEL_NAME, config);
    if (!g_daemon) {{ printf("❌ Failed to create channel\n"); return 1; }}
    
    printf("✅ Channel: %s | State: %zu bytes\n", {upper}_CHANNEL_NAME, sizeof({pascal}State));
    update_stats();
    printf("🔍 Detected %u CPU cores\n🚀 Publishing... (Ctrl+C to stop)\n\n", g_state.core_count);
    
    while (g_running) {{
        uint8_t cmd_buf[64];
        uint32_t client_id;
        while (venom_daemon_try_recv_command(g_daemon, cmd_buf, sizeof(cmd_buf), &client_id) > 0) {{
            printf("📥 Command from client %u\n", client_id);
        }}
        update_stats();
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
// Client
// ═══════════════════════════════════════════════════════════════════════════

fn client_main(config: &ProjectConfig) -> String {
    let upper = upper_name(&config.name);
    let pascal = pascal_case(&config.name);
    
    format!(r#"/**
 * {name} Status Bar - VenomMemory IPC Client
 * Displays live CPU/RAM stats with colored progress bars.
 * Includes read latency benchmarking.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <time.h>
#include <float.h>
#include "../shared/protocol.h"

typedef struct VenomShellHandle VenomShellHandle;
extern VenomShellHandle* venom_shell_connect(const char* name);
extern void venom_shell_destroy(VenomShellHandle* handle);
extern size_t venom_shell_read_data(VenomShellHandle* handle, uint8_t* buf, size_t max_len);
extern uint32_t venom_shell_id(VenomShellHandle* handle);

static VenomShellHandle* g_shell = NULL;
static volatile int g_running = 1;

// Latency tracking
static double g_latency_min = DBL_MAX;
static double g_latency_max = 0.0;
static double g_latency_sum = 0.0;
static uint64_t g_latency_count = 0;

static void signal_handler(int sig) {{ (void)sig; g_running = 0; }}

static double get_time_us(void) {{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000.0 + ts.tv_nsec / 1000.0;
}}

static void print_bar(float pct, int w) {{
    int filled = (int)((pct / 100.0f) * w);
    printf("[");
    for (int i = 0; i < w; i++) {{
        if (i < filled) {{
            if (pct > 80) printf("\033[91m█\033[0m");
            else if (pct > 50) printf("\033[93m▓\033[0m");
            else printf("\033[92m░\033[0m");
        }} else printf(" ");
    }}
    printf("]");
}}

int main(void) {{
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);
    
    printf("╔═══════════════════════════════════════════════════════════════╗\n");
    printf("║   🖥️  {name} Status Bar (C)                                   ║\n");
    printf("╚═══════════════════════════════════════════════════════════════╝\n\n");
    
    g_shell = venom_shell_connect({upper}_CHANNEL_NAME);
    if (!g_shell) {{
        printf("❌ Failed to connect! Run the daemon first:\n   cd ../daemon && make run\n");
        return 1;
    }}
    printf("✅ Connected! ID: %u\n📊 Reading stats... (Ctrl+C to exit)\n\n", venom_shell_id(g_shell));
    sleep(1);
    
    uint8_t* buf = malloc(sizeof({pascal}State) + 256);
    int frame = 0;
    
    while (g_running) {{
        // ═══════════════════════════════════════════════════════════════════
        // 📊 BENCHMARK: Measure read latency
        // ═══════════════════════════════════════════════════════════════════
        double t_start = get_time_us();
        size_t len = venom_shell_read_data(g_shell, buf, sizeof({pascal}State) + 256);
        double t_end = get_time_us();
        double latency_us = t_end - t_start;
        
        // Update stats
        if (latency_us < g_latency_min) g_latency_min = latency_us;
        if (latency_us > g_latency_max) g_latency_max = latency_us;
        g_latency_sum += latency_us;
        g_latency_count++;
        double avg_us = g_latency_sum / g_latency_count;
        
        if (len >= sizeof({pascal}State)) {{
            {pascal}State* s = ({pascal}State*)buf;
            if (s->magic != {upper}_MAGIC) {{ usleep(100000); continue; }}
            
            printf("\033[2J\033[H"); // Clear screen
            printf("╔═══════════════════════════════════════════════════════════════╗\n");
            printf("║  🖥️  {name} Monitor (C)            Frame: %-6d              ║\n", frame++);
            printf("╠═══════════════════════════════════════════════════════════════╣\n");
            printf("║  CPU: "); print_bar(s->cpu_usage_percent, 25); printf(" %5.1f%%             ║\n", s->cpu_usage_percent);
            printf("╠═══════════════════════════════════════════════════════════════╣\n");
            
            uint32_t show = s->core_count > 8 ? 8 : s->core_count;
            for (uint32_t i = 0; i < show; i++) {{
                printf("║  Core %u: ", i); print_bar(s->cpu_cores[i], 20); printf(" %5.1f%%                ║\n", s->cpu_cores[i]);
            }}
            if (s->core_count > 8) printf("║  ... +%u more cores                                            ║\n", s->core_count - 8);
            
            printf("╠═══════════════════════════════════════════════════════════════╣\n");
            float mem_pct = s->memory_total_mb > 0 ? (float)s->memory_used_mb / s->memory_total_mb * 100 : 0;
            printf("║  RAM: "); print_bar(mem_pct, 25); printf(" %u/%u MB          ║\n", s->memory_used_mb, s->memory_total_mb);
            printf("╠═══════════════════════════════════════════════════════════════╣\n");
            printf("║  ⏱️ Uptime: %lud %luh %lum                                      ║\n",
                (unsigned long)(s->uptime_seconds/86400), (unsigned long)((s->uptime_seconds%86400)/3600), (unsigned long)((s->uptime_seconds%3600)/60));
            printf("╠═══════════════════════════════════════════════════════════════╣\n");
            printf("║  📊 \033[96mRead Latency:\033[0m %.2f µs (min: %.2f, max: %.2f, avg: %.2f)  ║\n",
                latency_us, g_latency_min, g_latency_max, avg_us);
            printf("╚═══════════════════════════════════════════════════════════════╝\n");
            printf("  Cores: %u | Updates: %lu | Ctrl+C to exit\n", s->core_count, (unsigned long)s->update_counter);
        }}
        usleep(100000);
    }}
    
    // Print final stats
    printf("\n\n📊 \033[96mFinal Latency Stats (C):\033[0m\n");
    printf("   Samples: %lu\n", (unsigned long)g_latency_count);
    printf("   Min: %.2f µs\n", g_latency_min);
    printf("   Max: %.2f µs\n", g_latency_max);
    printf("   Avg: %.2f µs\n", g_latency_sum / g_latency_count);
    
    free(buf);
    venom_shell_destroy(g_shell);
    printf("\n👋 Goodbye!\n");
    return 0;
}}
"#, name = config.name, upper = upper, pascal = pascal)
}

fn client_makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Client Makefile

CC = gcc
CFLAGS = -Wall -Wextra -O2 -I../shared
LDFLAGS = -L../lib -lvenom_memory -Wl,-rpath,'$$ORIGIN/../lib'

TARGET = {name}_client
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

fn readme(config: &ProjectConfig) -> String {
    format!(r#"# {name}

Generated by VenomMemory CLI

## Quick Start

```bash
# Terminal 1 - Start daemon
cd daemon && make run

# Terminal 2 - Start client
cd client && make run
```

## Configuration

| Setting | Value |
|---------|-------|
| Channel | `{channel}` |
| Data Size | {data_size} bytes |
| Command Slots | {cmd_slots} |
| Max Clients | {max_clients} |

## Customization

Edit `shared/protocol.h` to modify the data structure.
"#,
        name = config.name,
        channel = config.channel,
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients
    )
}
