//! Go Templates for VenomMemory projects
//!
//! Generates a complete Go project with:
//! - CGO bindings for VenomMemory
//! - System monitor daemon
//! - Status bar client

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    crate::create_dir(&format!("{}/daemon", base));
    crate::create_dir(&format!("{}/client", base));
    
    // Daemon
    crate::write_file(&format!("{}/daemon/main.go", base), &daemon_main(config));
    
    // Client
    crate::write_file(&format!("{}/client/main.go", base), &client_main(config));
    
    // Shared bindings
    crate::write_file(&format!("{}/venom/venom.go", base), &venom_go(config));
    crate::create_dir(&format!("{}/venom", base));
    crate::write_file(&format!("{}/venom/venom.go", base), &venom_go(config));
    
    // go.mod
    crate::write_file(&format!("{}/go.mod", base), &go_mod(config));
    
    // Makefile
    crate::write_file(&format!("{}/Makefile", base), &makefile(config));
    
    // README
    crate::write_file(&format!("{}/README.md", base), &readme(config));
}

fn magic(channel: &str) -> u32 {
    channel.bytes().fold(0x564E4Fu32, |acc, b| acc.wrapping_add(b as u32))
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
// Venom bindings (Go + CGO)
// ═══════════════════════════════════════════════════════════════════════════

fn venom_go(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r##"package venom

/*
#cgo LDFLAGS: -L${{SRCDIR}}/../lib -lvenom_memory -Wl,-rpath,$ORIGIN/../lib
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>

typedef struct {{ size_t data_size; size_t cmd_slots; size_t max_clients; }} VenomConfig;
void* venom_daemon_create(const char* name, VenomConfig config);
void venom_daemon_destroy(void* handle);
void venom_daemon_write_data(void* handle, const uint8_t* data, size_t len);

void* venom_shell_connect(const char* name);
void venom_shell_destroy(void* handle);
size_t venom_shell_read_data(void* handle, uint8_t* buf, size_t max_len);
uint32_t venom_shell_id(void* handle);
*/
import "C"
import (
	"encoding/binary"
	"fmt"
	"unsafe"
)

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

const (
	ChannelName = "{channel}"
	Magic       = 0x{magic:08X}
	DataSize    = {data_size}
	CmdSlots    = {cmd_slots}
	MaxClients  = {max_clients}
	MaxCores    = 16
)

// ═══════════════════════════════════════════════════════════════════════════
// State Structure
// ═══════════════════════════════════════════════════════════════════════════

type {pascal}State struct {{
	Magic           uint32
	Version         uint32
	CPUUsagePercent float32
	CPUCores        [MaxCores]float32
	CoreCount       uint32
	MemoryUsedMB    uint32
	MemoryTotalMB   uint32
	UptimeSeconds   uint64
	UpdateCounter   uint64
	TimestampNs     uint64
}}

func (s *{pascal}State) IsValid() bool {{
	return s.Magic == Magic
}}

func (s *{pascal}State) MemoryPercent() float32 {{
	if s.MemoryTotalMB > 0 {{
		return float32(s.MemoryUsedMB) / float32(s.MemoryTotalMB) * 100
	}}
	return 0
}}

func (s *{pascal}State) UptimeFormatted() string {{
	h := s.UptimeSeconds / 3600
	m := (s.UptimeSeconds % 3600) / 60
	return fmt.Sprintf("%dh %dm", h, m)
}}

func (s *{pascal}State) ToBytes() []byte {{
	buf := make([]byte, 112)
	binary.LittleEndian.PutUint32(buf[0:], s.Magic)
	binary.LittleEndian.PutUint32(buf[4:], s.Version)
	copy(buf[8:12], (*[4]byte)(unsafe.Pointer(&s.CPUUsagePercent))[:])
	for i := 0; i < MaxCores; i++ {{
		copy(buf[12+i*4:16+i*4], (*[4]byte)(unsafe.Pointer(&s.CPUCores[i]))[:])
	}}
	binary.LittleEndian.PutUint32(buf[76:], s.CoreCount)
	binary.LittleEndian.PutUint32(buf[80:], s.MemoryUsedMB)
	binary.LittleEndian.PutUint32(buf[84:], s.MemoryTotalMB)
	binary.LittleEndian.PutUint64(buf[88:], s.UptimeSeconds)
	binary.LittleEndian.PutUint64(buf[96:], s.UpdateCounter)
	binary.LittleEndian.PutUint64(buf[104:], s.TimestampNs)
	return buf
}}

func StateFromBytes(data []byte) *{pascal}State {{
	if len(data) < 112 {{
		return &{pascal}State{{}}
	}}
	s := &{pascal}State{{}}
	s.Magic = binary.LittleEndian.Uint32(data[0:])
	s.Version = binary.LittleEndian.Uint32(data[4:])
	s.CPUUsagePercent = *(*float32)(unsafe.Pointer(&data[8]))
	for i := 0; i < MaxCores; i++ {{
		s.CPUCores[i] = *(*float32)(unsafe.Pointer(&data[12+i*4]))
	}}
	s.CoreCount = binary.LittleEndian.Uint32(data[76:])
	s.MemoryUsedMB = binary.LittleEndian.Uint32(data[80:])
	s.MemoryTotalMB = binary.LittleEndian.Uint32(data[84:])
	s.UptimeSeconds = binary.LittleEndian.Uint64(data[88:])
	s.UpdateCounter = binary.LittleEndian.Uint64(data[96:])
	s.TimestampNs = binary.LittleEndian.Uint64(data[104:])
	return s
}}

// ═══════════════════════════════════════════════════════════════════════════
// Daemon
// ═══════════════════════════════════════════════════════════════════════════

type Daemon struct {{
	handle unsafe.Pointer
}}

func NewDaemon() (*Daemon, error) {{
	name := C.CString(ChannelName)
	defer C.free(unsafe.Pointer(name))
	
	cfg := C.VenomConfig{{
		data_size:   C.size_t(DataSize),
		cmd_slots:   C.size_t(CmdSlots),
		max_clients: C.size_t(MaxClients),
	}}
	
	handle := C.venom_daemon_create(name, cfg)
	if handle == nil {{
		return nil, fmt.Errorf("failed to create daemon channel")
	}}
	return &Daemon{{handle: handle}}, nil
}}

func (d *Daemon) Write(state *{pascal}State) {{
	data := state.ToBytes()
	C.venom_daemon_write_data(d.handle, (*C.uint8_t)(&data[0]), C.size_t(len(data)))
}}

func (d *Daemon) Close() {{
	if d.handle != nil {{
		C.venom_daemon_destroy(d.handle)
		d.handle = nil
	}}
}}

// ═══════════════════════════════════════════════════════════════════════════
// Shell (Client)
// ═══════════════════════════════════════════════════════════════════════════

type Shell struct {{
	handle unsafe.Pointer
}}

func Connect() (*Shell, error) {{
	name := C.CString(ChannelName)
	defer C.free(unsafe.Pointer(name))
	
	handle := C.venom_shell_connect(name)
	if handle == nil {{
		return nil, fmt.Errorf("failed to connect - is daemon running?")
	}}
	return &Shell{{handle: handle}}, nil
}}

func (s *Shell) ClientID() uint32 {{
	return uint32(C.venom_shell_id(s.handle))
}}

func (s *Shell) ReadState() *{pascal}State {{
	buf := make([]byte, 256)
	n := C.venom_shell_read_data(s.handle, (*C.uint8_t)(&buf[0]), C.size_t(len(buf)))
	return StateFromBytes(buf[:n])
}}

func (s *Shell) Close() {{
	if s.handle != nil {{
		C.venom_shell_destroy(s.handle)
		s.handle = nil
	}}
}}
"##,
        channel = config.channel,
        magic = magic(&config.channel),
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients,
        pascal = pascal
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Daemon
// ═══════════════════════════════════════════════════════════════════════════

fn daemon_main(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r##"package main

import (
	"bufio"
	"fmt"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	"{name}/venom"
)

var prevTotal = make([]uint64, venom.MaxCores+1)
var prevIdle = make([]uint64, venom.MaxCores+1)

func readCPU(state *venom.{pascal}State) {{
	f, err := os.Open("/proc/stat")
	if err != nil {{
		return
	}}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	coreIdx := 0
	for scanner.Scan() && coreIdx <= venom.MaxCores {{
		line := scanner.Text()
		if !strings.HasPrefix(line, "cpu") {{
			continue
		}}
		fields := strings.Fields(line)
		if len(fields) < 8 {{
			continue
		}}

		user, _ := strconv.ParseUint(fields[1], 10, 64)
		nice, _ := strconv.ParseUint(fields[2], 10, 64)
		system, _ := strconv.ParseUint(fields[3], 10, 64)
		idle, _ := strconv.ParseUint(fields[4], 10, 64)
		iowait, _ := strconv.ParseUint(fields[5], 10, 64)
		irq, _ := strconv.ParseUint(fields[6], 10, 64)
		softirq, _ := strconv.ParseUint(fields[7], 10, 64)

		total := user + nice + system + idle + iowait + irq + softirq
		idleTime := idle + iowait
		totalD := total - prevTotal[coreIdx]
		idleD := idleTime - prevIdle[coreIdx]

		usage := float32(0)
		if totalD > 0 {{
			usage = (1.0 - float32(idleD)/float32(totalD)) * 100
		}}

		if fields[0] == "cpu" {{
			state.CPUUsagePercent = usage
		}} else if coreIdx > 0 && coreIdx <= venom.MaxCores {{
			state.CPUCores[coreIdx-1] = usage
		}}
		prevTotal[coreIdx] = total
		prevIdle[coreIdx] = idleTime
		coreIdx++
	}}
	state.CoreCount = uint32(coreIdx - 1)
}}

func readMemory(state *venom.{pascal}State) {{
	f, err := os.Open("/proc/meminfo")
	if err != nil {{
		return
	}}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	var totalKB, availKB uint64
	for scanner.Scan() {{
		line := scanner.Text()
		if strings.HasPrefix(line, "MemTotal:") {{
			fmt.Sscanf(line, "MemTotal: %d kB", &totalKB)
		}} else if strings.HasPrefix(line, "MemAvailable:") {{
			fmt.Sscanf(line, "MemAvailable: %d kB", &availKB)
		}}
	}}
	state.MemoryTotalMB = uint32(totalKB / 1024)
	state.MemoryUsedMB = uint32((totalKB - availKB) / 1024)
}}

func readUptime(state *venom.{pascal}State) {{
	data, err := os.ReadFile("/proc/uptime")
	if err != nil {{
		return
	}}
	var uptime float64
	fmt.Sscanf(string(data), "%f", &uptime)
	state.UptimeSeconds = uint64(uptime)
}}

func main() {{
	fmt.Println("🖥️  {name} System Monitor (Go)")
	fmt.Println("═══════════════════════════════════════════════════════════════")

	daemon, err := venom.NewDaemon()
	if err != nil {{
		fmt.Printf("❌ Error: %v\n", err)
		os.Exit(1)
	}}
	defer daemon.Close()

	fmt.Printf("✅ Channel: %s\n", venom.ChannelName)
	fmt.Println("🚀 Publishing... (Ctrl+C to stop)")

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	state := &venom.{pascal}State{{
		Magic:   venom.Magic,
		Version: 1,
	}}

	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()

	for {{
		select {{
		case <-sigCh:
			fmt.Println("\n\n👋 Goodbye!")
			return
		case <-ticker.C:
			readCPU(state)
			readMemory(state)
			readUptime(state)
			state.UpdateCounter++
			state.TimestampNs = uint64(time.Now().UnixNano())
			daemon.Write(state)

			fmt.Printf("\r🖥️  CPU: %.1f%% | RAM: %d/%d MB | #%d   ",
				state.CPUUsagePercent, state.MemoryUsedMB, state.MemoryTotalMB, state.UpdateCounter)
		}}
	}}
}}
"##, name = config.name, pascal = pascal)
}

// ═══════════════════════════════════════════════════════════════════════════
// Client
// ═══════════════════════════════════════════════════════════════════════════

fn client_main(config: &ProjectConfig) -> String {
    format!(r##"package main

import (
	"fmt"
	"math"
	"os"
	"os/signal"
	"syscall"
	"time"

	"{name}/venom"
)

const (
	Green  = "\033[92m"
	Yellow = "\033[93m"
	Red    = "\033[91m"
	Cyan   = "\033[96m"
	Reset  = "\033[0m"
)

// Latency tracking
var (
	latencyMin   = math.MaxFloat64
	latencyMax   = 0.0
	latencySum   = 0.0
	latencyCount uint64 = 0
)

func bar(pct float32, width int) string {{
	filled := int((pct / 100) * float32(width))
	color := Green
	if pct > 80 {{
		color = Red
	}} else if pct > 50 {{
		color = Yellow
	}}
	
	result := "["
	for i := 0; i < width; i++ {{
		if i < filled {{
			result += color + "█" + Reset
		}} else {{
			result += " "
		}}
	}}
	return result + "]"
}}

func main() {{
	fmt.Println("╔═══════════════════════════════════════════════════════════════╗")
	fmt.Println("║   🖥️  {name} Status Bar (Go)                                   ║")
	fmt.Println("╚═══════════════════════════════════════════════════════════════╝")
	fmt.Println()

	shell, err := venom.Connect()
	if err != nil {{
		fmt.Printf("❌ Error: %v\n", err)
		os.Exit(1)
	}}
	defer shell.Close()

	fmt.Printf("✅ Connected! ID: %d\n", shell.ClientID())
	fmt.Println("📊 Reading stats... (Ctrl+C to exit)")
	time.Sleep(1 * time.Second)

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()

	frame := 0
	for {{
		select {{
		case <-sigCh:
			// Print final stats
			fmt.Println("\n")
			fmt.Printf("📊 %sFinal Latency Stats (Go):%s\n", Cyan, Reset)
			fmt.Printf("   Samples: %d\n", latencyCount)
			fmt.Printf("   Min: %.2f µs\n", latencyMin)
			fmt.Printf("   Max: %.2f µs\n", latencyMax)
			fmt.Printf("   Avg: %.2f µs\n", latencySum/float64(latencyCount))
			fmt.Println("\n👋 Goodbye!")
			return
		case <-ticker.C:
			// ═══════════════════════════════════════════════════════════════════
			// 📊 BENCHMARK: Measure read latency
			// ═══════════════════════════════════════════════════════════════════
			tStart := time.Now()
			state := shell.ReadState()
			latencyUs := float64(time.Since(tStart).Nanoseconds()) / 1000.0
			
			// Update stats
			if latencyUs < latencyMin {{
				latencyMin = latencyUs
			}}
			if latencyUs > latencyMax {{
				latencyMax = latencyUs
			}}
			latencySum += latencyUs
			latencyCount++
			avgUs := latencySum / float64(latencyCount)
			
			if state.IsValid() {{
				fmt.Print("\033[2J\033[H")
				fmt.Println("╔═══════════════════════════════════════════════════════════════╗")
				fmt.Printf("║  🖥️  {name} Monitor (Go)         Frame: %-6d              ║\n", frame)
				fmt.Println("╠═══════════════════════════════════════════════════════════════╣")
				fmt.Printf("║  CPU: %s %5.1f%%             ║\n", bar(state.CPUUsagePercent, 25), state.CPUUsagePercent)
				fmt.Println("╠═══════════════════════════════════════════════════════════════╣")
				
				for i := uint32(0); i < state.CoreCount; i++ {{
					fmt.Printf("║  Core %d: %s %5.1f%%                ║\n", i, bar(state.CPUCores[i], 20), state.CPUCores[i])
				}}
				
				fmt.Println("╠═══════════════════════════════════════════════════════════════╣")
				fmt.Printf("║  RAM: %s %d/%d MB      ║\n", bar(state.MemoryPercent(), 25), state.MemoryUsedMB, state.MemoryTotalMB)
				fmt.Println("╠═══════════════════════════════════════════════════════════════╣")
				fmt.Printf("║  ⏱️ Uptime: %-40s ║\n", state.UptimeFormatted())
				fmt.Println("╠═══════════════════════════════════════════════════════════════╣")
				fmt.Printf("║  📊 %sRead Latency:%s %.2f µs (min: %.2f, max: %.2f, avg: %.2f)  ║\n", 
					Cyan, Reset, latencyUs, latencyMin, latencyMax, avgUs)
				fmt.Println("╚═══════════════════════════════════════════════════════════════╝")
				fmt.Printf("  Cores: %d | Updates: %d | Ctrl+C to exit\n", state.CoreCount, state.UpdateCounter)
				frame++
			}}
		}}
	}}
}}
"##, name = config.name)
}

fn go_mod(config: &ProjectConfig) -> String {
    format!(r#"module {name}

go 1.21
"#, name = config.name)
}

fn makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Go Project Makefile

.PHONY: all daemon client clean run-daemon run-client

all: daemon client

daemon:
	@echo "🔗 Building daemon..."
	@cd daemon && CGO_ENABLED=1 go build -o ../{name}_daemon .
	@echo "✅ Daemon built"

client:
	@echo "🔗 Building client..."
	@cd client && CGO_ENABLED=1 go build -o ../{name}_client .
	@echo "✅ Client built"

run-daemon: daemon
	@LD_LIBRARY_PATH=./lib ./{name}_daemon

run-client: client
	@LD_LIBRARY_PATH=./lib ./{name}_client

clean:
	@rm -f {name}_daemon {name}_client
"#, name = config.name)
}

fn readme(config: &ProjectConfig) -> String {
    format!(r#"# {name} (Go)

VenomMemory Go system monitor with CGO bindings.

## Quick Start

```bash
# Build
make all

# Terminal 1 - Daemon
make run-daemon

# Terminal 2 - Client
make run-client
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
