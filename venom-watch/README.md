# 🕵️ Venom Watch

**Venom Watch** is a powerful ABI (Application Binary Interface) validation tool designed to ensure memory safety and layout consistency in IPC (Inter-Process Communication) systems. It is specifically built to validate structures and enums shared between different languages and components (e.g., C and Rust).

## 🚀 Key Features

### 1. Cross-Language ABI Validation
- **C to C**: Compare layouts between system headers (`.h`) and application source code (`.c`).
- **Rust to C**: Directly validate Rust `#[repr(C)]` structs against C structures.
- **Smart Parsing**: Uses `tree-sitter` for high-precision parsing of both C and Rust syntax.

### 2. Implicit Padding Detection 🕵️‍♂️
- **Zero-Baud Awareness**: Identifies "invisible" bytes (padding) that compilers insert for memory alignment.
- **Gap Analysis**: Detects internal and trailing padding to ensure synchronization between different compiler versions or flags.

### 3. Deep Structural Comparison
- **Size Verification**: Validates total byte counts of structures.
- **Offset Tracking**: Ensures every field starts at the exact same memory location.
- **Pointer Danger Detection**: Flags fields involving pointers that might cause segmentation faults if mismanaged across memory boundaries.
- **Enum Consistency**: Checks for matching member names and values across definitions.

### 4. Developer-Friendly Output
- **Line Number Reporting**: Pinpoints exactly where each field or member is defined.
- **Color-Coded Feedback**: Instant visual confirmation (Green = Match, Red = Mismatch, Yellow = Warning).
- **Table View**: Comprehensive side-by-side comparison of layouts.

### 5. CI/CD & Automation Support 🚀
- **Automated Guardrails**: Returns a non-zero exit code (1) if validation fails, preventing broken ABIs from being merged.
- **Pipeline Integration**: Perfect for use as a pre-commit hook or as a step in a GitHub Actions workflow.

---

## 🛠️ Installation

Ensure you have Rust and Cargo installed, then build the project:

```bash
cd venom-watch
cargo build --release
```

---

## 📖 Usage Guide

### Validating Structures
To compare a structure between two files, provide the server file, client file, and the struct name:

```bash
./target/release/venom-watch \
  --server path/to/daemon_ipc.h \
  --client path/to/client_app.c \
  --struct-name MySharedState
```

### Validating Enums
To ensure your command codes or state enums match:

```bash
./target/release/venom-watch \
  --server path/to/daemon_ipc.h \
  --client path/to/client_app.c \
  --enum-name MyCommandEnum
```

### Example: Rust back-end to C front-end
If your daemon is in Rust and your client is in C:

```bash
./target/release/venom-watch \
  --server src/bindings.rs \
  --client include/api.h \
  --struct-name ConfigData
```

---

## 📊 Output Explanation

- **✅ OK**: Field matches perfectly in offset, size, and name.
- **⚠️ Name Diff**: Offset and size match, but the field has a different name (ABI stays safe, but might be confusing).
- **❌ Offset Mismatch**: Critical failure! Data will be read from the wrong location.
- **🚨 POINTER DANGER!**: Special warning for fields containing pointers which are not serializable in raw shared memory.
- **[PADDING]**: Highlights internal memory gaps added by the compiler.


- **Test set**

🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 32 bytes
Client Struct: 28 bytes
--------------------------------------------------
⚠️  SIZE MISMATCH IDENTIFIED!
Expected: 32 bytes
Found:    28 bytes

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
a                    | @0    (L3)       | @0    (L5)       | ✅ OK
[PADDING]            | 3 bytes          |                  | INTERNAL
[PADDING]            |                  | 3 bytes          | INTERNAL
b                    | @4    (L4)       | @4    (L6)       | ✅ OK
c                    | @8    (L5)       | @8    (L7)       | ✅ OK
d                    | @24   (L6)       | @24   (L8)       | ❌ Size Mismatch | 🚨 POINTER DANGER!

-------------------------------------------------------------------------------------------
🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 32 bytes
Client Struct: 32 bytes
--------------------------------------------------
✅ Total sizes match.

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
a                    | @0    (L3)       | @0    (L5)       | ✅ OK
[PADDING]            | 3 bytes          |                  | INTERNAL
[PADDING]            |                  | 3 bytes          | INTERNAL
b                    | @4    (L4)       | @4    (L6)       | ✅ OK
c                    | @8    (L5)       | @8    (L7)       | ✅ OK
d                    | @24   (L6)       | @24   (L8)       | ⚠️ Name Diff | 🚨 POINTER DANGER!
------------------------------------------------------------------------------------------
🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 32 bytes
Client Struct: 32 bytes
--------------------------------------------------
✅ Total sizes match.

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
a                    | @0    (L3)       | @0    (L5)       | ✅ OK
[PADDING]            | 3 bytes          |                  | INTERNAL
[PADDING]            |                  | 3 bytes          | INTERNAL
b                    | @4    (L4)       | @4    (L6)       | ✅ OK
c                    | @8    (L5)       | @8    (L7)       | ✅ OK
d                    | @24   (L6)       | @24   (L8)       | ✅ OK | 🚨 POINTER DANGER!
------------------------------------------------------------------------------------------
🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 13240 bytes
Client Struct: 13240 bytes
--------------------------------------------------
✅ Total sizes match.

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
magic                | @0    (L38)      | @0    (L56)      | ✅ OK
version              | @4    (L39)      | @4    (L57)      | ✅ OK
volume               | @8    (L42)      | @8    (L58)      | ✅ OK
mic_volume           | @12   (L43)      | @12   (L59)      | ✅ OK
muted                | @16   (L44)      | @16   (L60)      | ✅ OK
mic_muted            | @17   (L45)      | @17   (L61)      | ✅ OK
overamplification    | @18   (L46)      | @18   (L62)      | ✅ OK
_pad1                | @19   (L47)      | @19   (L63)      | ✅ OK
max_volume           | @20   (L48)      | @20   (L64)      | ✅ OK
default_sink         | @24   (L51)      | @24   (L65)      | ✅ OK
default_source       | @152  (L52)      | @152  (L66)      | ✅ OK
sink_count           | @280  (L55)      | @280  (L67)      | ✅ OK
sinks                | @284  (L56)      | @284  (L68)      | ✅ OK
source_count         | @4508 (L59)      | @4508 (L69)      | ✅ OK
sources              | @4512 (L60)      | @4512 (L70)      | ✅ OK
app_count            | @8736 (L63)      | @8736 (L71)      | ✅ OK
apps                 | @8740 (L64)      | @8740 (L72)      | ✅ OK
[PADDING]            | 4 bytes          |                  | INTERNAL
[PADDING]            |                  | 4 bytes          | INTERNAL
update_counter       | @13224 (L67)     | @13224 (L73)     | ✅ OK
timestamp_ns         | @13232 (L68)     | @13232 (L74)     | ✅ OK
------------------------------------------------------------------------------------------
x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ ./target/debug/venom-watch --server /home/x/Desktop/venom_memory_rs/venom_audio/include/venom_ipc.h --client /home/x/Desktop/venom_memory_rs/venom_audio/audio_client.c --struct-name VenomAudioState
🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 13240 bytes
Client Struct: 13240 bytes
--------------------------------------------------
✅ Total sizes match.

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
magic                | @0    (L38)      | @0    (L56)      | ✅ OK
version              | @4    (L39)      | @4    (L57)      | ✅ OK
volume               | @8    (L42)      | @8    (L58)      | ✅ OK
mic_volume           | @12   (L43)      | @12   (L59)      | ✅ OK
muted                | @16   (L44)      | @16   (L60)      | ✅ OK
mic_muted            | @17   (L45)      | @17   (L61)      | ✅ OK
overamplification    | @18   (L46)      | @18   (L62)      | ✅ OK
_pad1                | @19   (L47)      | @19   (L63)      | ✅ OK
max_volume           | @20   (L48)      | @20   (L64)      | ✅ OK
default_sink         | @24   (L51)      | @24   (L65)      | ✅ OK
default_source       | @152  (L52)      | @152  (L66)      | ✅ OK
sink_count           | @280  (L55)      | @280  (L67)      | ✅ OK
sinks                | @284  (L56)      | @284  (L68)      | ✅ OK
source_count         | @4508 (L59)      | @4508 (L69)      | ✅ OK
sources              | @4512 (L60)      | @4512 (L70)      | ✅ OK
app_count            | @8736 (L63)      | @8736 (L71)      | ✅ OK
apps                 | @8740 (L64)      | @8740 (L72)      | ✅ OK
update_counter       | @13224 (L67)     | @13224 (L73)     | ✅ OK
timestamp_ns         | @13232 (L68)     | @13232 (L74)     | ✅ OK
x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ ./target/debug/venom-watch --server /home/x/Desktop/venom_memory_rs/venom_audio/include/venom_ipc.h --client /home/x/Desktop/venom_memory_rs/venom_audio/audio_client.c --struct-name VenomAudioCommand
🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 8 bytes
Client Struct: 8 bytes
--------------------------------------------------
✅ Total sizes match.

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
cmd                  | @0    (L93)      | @0    (L96)      | ✅ OK
_pad                 | @1    (L94)      | @1    (L97)      | ✅ OK
data                 | @4    (L105)     | @4    (L108)     | ✅ OK
x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ ./target/debug/venom-watch --server /home/x/Desktop/venom_memory_rs/venom_audio/include/venom_ipc.h --client /home/x/Desktop/venom_memory_rs/venom_audio/audio_client.c --enum-name VenomAudioCmd
🕵️ Venom Watch: Validating IPC Structures...

Server Enum: 14 members
Client Enum: 14 members
--------------------------------------------------
Member                    | Server (Val)    | Client (Val)    | Status              
--------------------------------------------------------------------------------
CMD_SET_VOLUME            | 1 (L76)         | 1 (L79)         | ✅ OK
CMD_SET_MUTED             | 2 (L77)         | 2 (L80)         | ✅ OK
CMD_SET_MIC_VOLUME        | 3 (L78)         | 3 (L81)         | ✅ OK
CMD_SET_MIC_MUTED         | 4 (L79)         | 4 (L82)         | ✅ OK
CMD_SET_DEFAULT_SINK      | 5 (L80)         | 5 (L83)         | ✅ OK
CMD_SET_DEFAULT_SOURCE    | 6 (L81)         | 6 (L84)         | ✅ OK
CMD_SET_SINK_VOLUME       | 7 (L82)         | 7 (L85)         | ✅ OK
CMD_SET_SOURCE_VOLUME     | 8 (L83)         | 8 (L86)         | ✅ OK
CMD_SET_APP_VOLUME        | 9 (L84)         | 9 (L87)         | ✅ OK
CMD_SET_APP_MUTED         | 10 (L85)        | 10 (L88)        | ✅ OK
CMD_MOVE_APP_TO_SINK      | 11 (L86)        | 11 (L89)        | ✅ OK
CMD_SET_OVERAMPLIFICATION | 12 (L87)        | 12 (L90)        | ✅ OK
CMD_SET_PROFILE           | 13 (L88)        | 13 (L91)        | ✅ OK
CMD_REFRESH               | 14 (L89)        | 14 (L92)        | ✅ OK

✅ Enums are fully consistent!

------------------------------------------------------------------------------------------

x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ cargo build && ./target/debug/venom-watch --server /home/x/Desktop/venom_memory_rs/venom_audio/include/venom_ipc.h --client /home/x/Desktop/venom_memory_rs/venom_audio/audio_client.c --struct-name VenomAudioState && echo "Exit code: $?"
   Compiling venom-watch v0.1.0 (/home/x/Desktop/venom_memory_rs/venom-watch)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s
🕵️ Venom Watch: Validating IPC Structures...

Server Struct: 13240 bytes
Client Struct: 13240 bytes
--------------------------------------------------
✅ Total sizes match.

Field                | Server (Line)    | Client (Line)    | Status                        
------------------------------------------------------------------------------------------
magic                | @0    (L38)      | @0    (L56)      | ✅ OK
version              | @4    (L39)      | @4    (L57)      | ✅ OK
volume               | @8    (L42)      | @8    (L58)      | ✅ OK
mic_volume           | @12   (L43)      | @12   (L59)      | ✅ OK
muted                | @16   (L44)      | @16   (L60)      | ✅ OK
mic_muted            | @17   (L45)      | @17   (L61)      | ✅ OK
overamplification    | @18   (L46)      | @18   (L62)      | ✅ OK
_pad1                | @19   (L47)      | @19   (L63)      | ✅ OK
max_volume           | @20   (L48)      | @20   (L64)      | ✅ OK
default_sink         | @24   (L51)      | @24   (L65)      | ✅ OK
default_source       | @152  (L52)      | @152  (L66)      | ✅ OK
sink_count           | @280  (L55)      | @280  (L67)      | ✅ OK
sinks                | @284  (L56)      | @284  (L68)      | ✅ OK
source_count         | @4508 (L59)      | @4508 (L69)      | ✅ OK
sources              | @4512 (L60)      | @4512 (L70)      | ✅ OK
app_count            | @8736 (L63)      | @8736 (L71)      | ✅ OK
apps                 | @8740 (L64)      | @8740 (L72)      | ✅ OK
[PADDING]            | 4 bytes          |                  | INTERNAL
[PADDING]            |                  | 4 bytes          | INTERNAL
update_counter       | @13224 (L67)     | @13224 (L73)     | ✅ OK
timestamp_ns         | @13232 (L68)     | @13232 (L74)     | ✅ OK
Exit code: 0
x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ cd /home/x/Desktop/venom_memory_rs/venom-watch/
x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ ./target/debug/venom-watch --server /home/x/Desktop/venom_memory_rs/venom_audio/include/venom_ipc.h --client /home/x/Desktop/venom_memory_rs/venom_audio/audio_client.c --struct-name NonExistentStruct && echo "Exit code: $?" || echo "Exit code: $?"
🕵️ Venom Watch: Validating IPC Structures...
❌ Server struct analysis failed: Struct 'NonExistentStruct' not found in /home/x/Desktop/venom_memory_rs/venom_audio/include/venom_ipc.h
Exit code: 1
x@x-HP-ZBook-15u-G6:~/Desktop/venom_memory_rs/venom-watch$ 