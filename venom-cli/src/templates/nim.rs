//! Nim Templates for VenomMemory projects
//!
//! Generates a complete Nim project with:
//! - C FFI bindings
//! - System monitor daemon
//! - Status bar client

use super::ProjectConfig;

pub fn generate(config: &ProjectConfig) {
    let base = &config.output_dir;
    
    crate::create_dir(&format!("{}/src", base));
    
    // Main source files
    crate::write_file(&format!("{}/src/venom.nim", base), &venom_nim(config));
    crate::write_file(&format!("{}/src/daemon.nim", base), &daemon_nim(config));
    crate::write_file(&format!("{}/src/client.nim", base), &client_nim(config));
    
    // Config file
    crate::write_file(&format!("{}/{}.nimble", base, config.name), &nimble(config));
    
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
// Venom bindings (Nim)
// ═══════════════════════════════════════════════════════════════════════════

fn venom_nim(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r##"## VenomMemory Nim Bindings

import os, strformat

# ═══════════════════════════════════════════════════════════════════════════
# Configuration
# ═══════════════════════════════════════════════════════════════════════════

const
  ChannelName* = "{channel}"
  Magic*: uint32 = 0x{magic:08X}'u32
  DataSize* = {data_size}
  CmdSlots* = {cmd_slots}
  MaxClients* = {max_clients}
  MaxCores* = 16

# ═══════════════════════════════════════════════════════════════════════════
# State Structure (packed to match C layout)
# ═══════════════════════════════════════════════════════════════════════════

type
  {pascal}State* {{.packed.}} = object
    magic*: uint32
    version*: uint32
    cpuUsagePercent*: float32
    cpuCores*: array[MaxCores, float32]
    coreCount*: uint32
    memoryUsedMB*: uint32
    memoryTotalMB*: uint32
    uptimeSeconds*: uint64
    updateCounter*: uint64
    timestampNs*: uint64

proc isValid*(s: {pascal}State): bool = s.magic == Magic

proc memoryPercent*(s: {pascal}State): float32 =
  if s.memoryTotalMB > 0:
    return float32(s.memoryUsedMB) / float32(s.memoryTotalMB) * 100.0
  return 0

proc uptimeFormatted*(s: {pascal}State): string =
  let h = s.uptimeSeconds div 3600
  let m = (s.uptimeSeconds mod 3600) div 60
  return fmt"{{h}}h {{m}}m"

static:
  assert sizeof({pascal}State) == 112, "State size mismatch"

# ═══════════════════════════════════════════════════════════════════════════
# C FFI Bindings
# ═══════════════════════════════════════════════════════════════════════════

# C FFI Bindings (library path set via Makefile passL)

type
  VenomConfig {{.packed.}} = object
    data_size: csize_t
    cmd_slots: csize_t
    max_clients: csize_t

proc venom_daemon_create(name: cstring, config: VenomConfig): pointer {{.importc, cdecl.}}
proc venom_daemon_destroy(handle: pointer) {{.importc, cdecl.}}
proc venom_daemon_write_data(handle: pointer, data: ptr uint8, len: csize_t) {{.importc, cdecl.}}

proc venom_shell_connect(name: cstring): pointer {{.importc, cdecl.}}
proc venom_shell_destroy(handle: pointer) {{.importc, cdecl.}}
proc venom_shell_read_data(handle: pointer, buf: ptr uint8, maxLen: csize_t): csize_t {{.importc, cdecl.}}
proc venom_shell_id(handle: pointer): uint32 {{.importc, cdecl.}}

# ═══════════════════════════════════════════════════════════════════════════
# Daemon Wrapper
# ═══════════════════════════════════════════════════════════════════════════

type Daemon* = object
  handle: pointer

proc newDaemon*(): Daemon =
  let cfg = VenomConfig(
    data_size: DataSize.csize_t,
    cmd_slots: CmdSlots.csize_t,
    max_clients: MaxClients.csize_t
  )
  let h = venom_daemon_create(ChannelName.cstring, cfg)
  if h == nil:
    raise newException(IOError, "Failed to create daemon channel")
  result.handle = h

proc write*(d: Daemon, state: {pascal}State) =
  var s = state
  venom_daemon_write_data(d.handle, cast[ptr uint8](addr s), csize_t(sizeof(s)))

proc close*(d: Daemon) =
  if d.handle != nil:
    venom_daemon_destroy(d.handle)

# ═══════════════════════════════════════════════════════════════════════════
# Shell (Client) Wrapper
# ═══════════════════════════════════════════════════════════════════════════

type Shell* = object
  handle: pointer

proc connect*(): Shell =
  let h = venom_shell_connect(ChannelName.cstring)
  if h == nil:
    raise newException(IOError, "Failed to connect - is daemon running?")
  result.handle = h

proc clientId*(s: Shell): uint32 =
  return venom_shell_id(s.handle)

proc readState*(s: Shell): {pascal}State =
  var buf: array[256, uint8]
  let n = venom_shell_read_data(s.handle, addr buf[0], csize_t(buf.len))
  if n >= csize_t(sizeof(result)):
    copyMem(addr result, addr buf[0], sizeof(result))

proc close*(s: Shell) =
  if s.handle != nil:
    venom_shell_destroy(s.handle)
"##,
        channel = config.channel,
        magic = magic(&config.channel),
        data_size = config.data_size,
        cmd_slots = config.cmd_slots,
        max_clients = config.max_clients,
        pascal = pascal
    )
}

fn daemon_nim(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r##"## {name} System Monitor Daemon (Nim)

import os, strformat, strutils, times
import venom

var prevTotal: array[venom.MaxCores + 1, uint64]
var prevIdle: array[venom.MaxCores + 1, uint64]

proc readCpu(state: var {pascal}State) =
  let f = open("/proc/stat")
  defer: f.close()
  
  var coreIdx = 0
  for line in f.lines:
    if coreIdx > venom.MaxCores: break
    if not line.startsWith("cpu"): continue
    
    let parts = line.splitWhitespace()
    if parts.len < 8: continue
    
    let user = parseUInt(parts[1])
    let nice = parseUInt(parts[2])
    let system = parseUInt(parts[3])
    let idle = parseUInt(parts[4])
    let iowait = parseUInt(parts[5])
    let irq = parseUInt(parts[6])
    let softirq = parseUInt(parts[7])
    
    let total = user + nice + system + idle + iowait + irq + softirq
    let idleTime = idle + iowait
    let totalD = total - prevTotal[coreIdx]
    let idleD = idleTime - prevIdle[coreIdx]
    
    let usage = if totalD > 0: (1.0 - float32(idleD) / float32(totalD)) * 100.0 else: 0.0
    
    if parts[0] == "cpu":
      state.cpuUsagePercent = usage
    elif coreIdx > 0 and coreIdx <= venom.MaxCores:
      state.cpuCores[coreIdx - 1] = usage
    
    prevTotal[coreIdx] = total
    prevIdle[coreIdx] = idleTime
    coreIdx.inc
  
  state.coreCount = uint32(if coreIdx > 1: coreIdx - 1 else: 0)

proc readMemory(state: var {pascal}State) =
  let f = open("/proc/meminfo")
  defer: f.close()
  
  var totalKb, availKb: uint64
  for line in f.lines:
    let parts = line.splitWhitespace()
    if parts.len >= 2:
      if parts[0] == "MemTotal:":
        totalKb = parseUInt(parts[1])
      elif parts[0] == "MemAvailable:":
        availKb = parseUInt(parts[1])
  
  state.memoryTotalMB = uint32(totalKb div 1024)
  state.memoryUsedMB = uint32((totalKb - availKb) div 1024)

proc readUptime(state: var {pascal}State) =
  let content = readFile("/proc/uptime")
  let uptimeStr = content.split()[0]
  let dotIdx = uptimeStr.find('.')
  let uptimeSec = if dotIdx >= 0: uptimeStr[0..<dotIdx] else: uptimeStr
  state.uptimeSeconds = parseUInt(uptimeSec)

proc main() =
  echo "🖥️  {name} System Monitor (Nim)"
  echo "═══════════════════════════════════════════════════════════════"
  
  let daemon = newDaemon()
  defer: daemon.close()
  
  echo fmt"✅ Channel: {{venom.ChannelName}}"
  echo "🚀 Publishing... (Ctrl+C to stop)"
  echo ""
  
  var state = {pascal}State(
    magic: venom.Magic,
    version: 1
  )
  
  while true:
    readCpu(state)
    readMemory(state)
    readUptime(state)
    state.updateCounter.inc
    state.timestampNs = uint64(epochTime() * 1_000_000_000)
    
    daemon.write(state)
    
    stdout.write fmt"\r🖥️  CPU: {{state.cpuUsagePercent:.1f}}% | RAM: {{state.memoryUsedMB}}/{{state.memoryTotalMB}} MB | #{{state.updateCounter}}   "
    stdout.flushFile()
    
    sleep(100)

when isMainModule:
  main()
"##, name = config.name, pascal = pascal)
}

fn client_nim(config: &ProjectConfig) -> String {
    let pascal = pascal_case(&config.name);
    
    format!(r##"## {name} Status Bar Client (Nim) - with Benchmarking

import os, strformat, terminal, times
import venom

const
  Green = "\e[92m"
  Yellow = "\e[93m"
  Red = "\e[91m"
  Cyan = "\e[96m"
  Reset = "\e[0m"

# Latency tracking
var
  latencyMin = float.high
  latencyMax = 0.0
  latencySum = 0.0
  latencyCount: uint64 = 0

proc printBar(pct: float32, width: int = 25): string =
  let filled = int((pct / 100.0) * float32(width))
  let color = if pct > 80: Red elif pct > 50: Yellow else: Green
  
  result = "["
  for i in 0..<width:
    if i < filled:
      result &= color & "█" & Reset
    else:
      result &= " "
  result &= "]"

proc main() =
  echo "╔═══════════════════════════════════════════════════════════════╗"
  echo "║   🖥️  {name} Status Bar (Nim)                                  ║"
  echo "╚═══════════════════════════════════════════════════════════════╝"
  echo ""
  
  let shell = connect()
  defer: shell.close()
  
  echo fmt"✅ Connected! ID: {{shell.clientId()}}"
  echo "📊 Reading stats... (Ctrl+C to exit)"
  echo ""
  sleep(1000)
  
  var frame = 0
  while true:
    # ═══════════════════════════════════════════════════════════════════
    # 📊 BENCHMARK: Measure read latency
    # ═══════════════════════════════════════════════════════════════════
    let tStart = cpuTime()
    let state = shell.readState()
    let tEnd = cpuTime()
    let latencyUs = (tEnd - tStart) * 1_000_000
    
    # Update stats
    if latencyUs < latencyMin: latencyMin = latencyUs
    if latencyUs > latencyMax: latencyMax = latencyUs
    latencySum += latencyUs
    latencyCount.inc
    let avgUs = latencySum / float(latencyCount)
    
    if state.isValid():
      eraseScreen()
      setCursorPos(0, 0)
      
      echo "╔═══════════════════════════════════════════════════════════════╗"
      echo fmt"║  🖥️  {name} Monitor (Nim)         Frame: {{frame:<6}}              ║"
      echo "╠═══════════════════════════════════════════════════════════════╣"
      echo fmt"║  CPU: {{printBar(state.cpuUsagePercent)}} {{state.cpuUsagePercent:5.1f}}%             ║"
      echo "╠═══════════════════════════════════════════════════════════════╣"
      
      for i in 0..<int(state.coreCount):
        echo fmt"║  Core {{i}}: {{printBar(state.cpuCores[i], 20)}} {{state.cpuCores[i]:5.1f}}%                ║"
      
      echo "╠═══════════════════════════════════════════════════════════════╣"
      echo fmt"║  RAM: {{printBar(state.memoryPercent())}} {{state.memoryUsedMB}}/{{state.memoryTotalMB}} MB      ║"
      echo "╠═══════════════════════════════════════════════════════════════╣"
      echo fmt"║  ⏱️ Uptime: {{state.uptimeFormatted()}}                                        ║"
      echo "╠═══════════════════════════════════════════════════════════════╣"
      echo fmt"║  📊 {{Cyan}}Read Latency:{{Reset}} {{latencyUs:.2f}} µs (min: {{latencyMin:.2f}}, max: {{latencyMax:.2f}}, avg: {{avgUs:.2f}})  ║"
      echo "╚═══════════════════════════════════════════════════════════════╝"
      echo fmt"  Cores: {{state.coreCount}} | Updates: {{state.updateCounter}} | Ctrl+C to exit"
      
      frame.inc
    
    sleep(100)

when isMainModule:
  main()
"##, name = config.name)
}

fn nimble(config: &ProjectConfig) -> String {
    format!(r#"# {name} Nimble Package

version       = "0.1.0"
author        = "VenomMemory"
description   = "System monitor using VenomMemory"
license       = "MIT"
srcDir        = "src"
bin           = @["{name}_daemon", "{name}_client"]

requires "nim >= 1.6.0"
"#, name = config.name)
}

fn makefile(config: &ProjectConfig) -> String {
    format!(r#"# {name} Nim Project Makefile

.PHONY: all daemon client clean run-daemon run-client

all: daemon client

daemon:
	@echo "🔗 Building daemon..."
	@nim c --passL:"-L./lib -lvenom_memory -Wl,-rpath,\$$ORIGIN/lib" -o:{name}_daemon src/daemon.nim
	@echo "✅ Daemon built"

client:
	@echo "🔗 Building client..."
	@nim c --passL:"-L./lib -lvenom_memory -Wl,-rpath,\$$ORIGIN/lib" -o:{name}_client src/client.nim
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
    format!(r#"# {name} (Nim)

VenomMemory Nim system monitor with C FFI bindings.

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
