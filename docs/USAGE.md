# 📚 VenomMemory Usage Guide

دليل شامل لاستخدام مكتبة VenomMemory للتواصل بين العمليات (IPC).

---

## 🎯 نظرة عامة

VenomMemory هي مكتبة IPC عالية الأداء تستخدم الذاكرة المشتركة. تعتمد على نموذج **Daemon-Shell**:

| المكون | الدور | العمليات |
|--------|------|----------|
| **Daemon** | الخادم/الكاتب | إنشاء القناة، كتابة البيانات، استقبال الأوامر |
| **Shell** | العميل/القارئ | الاتصال بالقناة، قراءة البيانات، إرسال الأوامر |

```
┌─────────────────────────────────────────────────────────────┐
│                        DAEMON                               │
│  ┌─────────────────┐    ┌─────────────────┐                │
│  │  write_data()   │──▶│   try_recv()    │◀─ أوامر        │
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

## 📦 التثبيت

### Cargo.toml (Rust)
```toml
[dependencies]
venom_memory = { path = "../venom_memory_rs" }
```

### C/C++
```bash
# نسخ الملفات
cp target/release/libvenom_memory.so /usr/local/lib/
cp venom_memory_rs.h /usr/local/include/

# الربط
gcc -o myapp myapp.c -lvenom_memory
```

### Flutter/Dart
```yaml
# pubspec.yaml
dependencies:
  ffi: ^2.1.0
```

---

## 🔧 الاستخدام الأساسي (Rust)

### 1️⃣ إنشاء Daemon (الخادم)

```rust
use venom_memory::{DaemonChannel, ChannelConfig};

fn main() {
    // تكوين القناة
    let config = ChannelConfig {
        data_size: 1024,      // حجم البيانات (بايت)
        cmd_slots: 16,        // عدد خانات الأوامر
        max_clients: 8,       // أقصى عدد للعملاء
    };

    // إنشاء القناة
    let daemon = DaemonChannel::create("my_channel", config)
        .expect("فشل إنشاء القناة");

    println!("✅ تم إنشاء القناة: my_channel");

    loop {
        // كتابة البيانات
        let data = b"Hello from daemon!";
        daemon.write_data(data);

        // استقبال الأوامر (غير محجوب)
        let mut cmd_buf = [0u8; 64];
        if let Some((client_id, len)) = daemon.try_recv_command(&mut cmd_buf) {
            let cmd = String::from_utf8_lossy(&cmd_buf[..len]);
            println!("📥 أمر من العميل {}: {}", client_id, cmd);
            
            // معالجة الأمر
            match cmd.as_ref() {
                "PING" => println!("PONG!"),
                "STOP" => break,
                _ => println!("أمر غير معروف"),
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
```

### 2️⃣ إنشاء Shell (العميل)

```rust
use venom_memory::ShellChannel;

fn main() {
    // الاتصال بالقناة
    let shell = ShellChannel::connect("my_channel")
        .expect("فشل الاتصال");

    println!("✅ متصل! معرف العميل: {}", shell.client_id());

    // إرسال أمر للخادم
    shell.try_send_command(b"PING");
    println!("📤 تم إرسال PING");

    // قراءة البيانات
    let mut buf = vec![0u8; 1024];
    loop {
        let len = shell.read_data(&mut buf);
        if len > 0 {
            let data = String::from_utf8_lossy(&buf[..len]);
            println!("📥 بيانات: {}", data);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
```

---

## 📊 نقل البيانات المهيكلة (Structs)

### تعريف الهيكل المشترك

```rust
// يجب أن يكون متطابقاً في الخادم والعميل!
#[repr(C)]  // مهم جداً للتوافق مع C
#[derive(Clone, Copy, Default)]
pub struct SensorData {
    pub temperature: f32,
    pub humidity: f32,
    pub pressure: f32,
    pub timestamp: u64,
}
```

### الكتابة (Daemon)

```rust
let data = SensorData {
    temperature: 25.5,
    humidity: 60.0,
    pressure: 1013.25,
    timestamp: 1234567890,
};

// تحويل الهيكل إلى بايتات
let bytes = unsafe {
    std::slice::from_raw_parts(
        &data as *const SensorData as *const u8,
        std::mem::size_of::<SensorData>()
    )
};

daemon.write_data(bytes);
```

### القراءة (Shell)

```rust
let mut buf = vec![0u8; std::mem::size_of::<SensorData>() + 64];
let len = shell.read_data(&mut buf);

if len >= std::mem::size_of::<SensorData>() {
    let data: SensorData = unsafe {
        std::ptr::read(buf.as_ptr() as *const SensorData)
    };
    println!("🌡️ درجة الحرارة: {}°C", data.temperature);
}
```

---

## 🔌 الاستخدام من C

### الهيدر (venom_memory_rs.h)

```c
// الأنواع
typedef struct VenomDaemonHandle VenomDaemonHandle;
typedef struct VenomShellHandle VenomShellHandle;

typedef struct {
    size_t data_size;
    size_t cmd_slots;
    size_t max_clients;
} VenomConfig;

// دوال الخادم
VenomDaemonHandle* venom_daemon_create(const char* name, VenomConfig config);
void venom_daemon_destroy(VenomDaemonHandle* handle);
void venom_daemon_write_data(VenomDaemonHandle* handle, const uint8_t* data, size_t len);

// دوال العميل
VenomShellHandle* venom_shell_connect(const char* name);
void venom_shell_destroy(VenomShellHandle* handle);
size_t venom_shell_read_data(VenomShellHandle* handle, uint8_t* buf, size_t max_len);
uint32_t venom_shell_id(VenomShellHandle* handle);
bool venom_shell_send_command(VenomShellHandle* handle, const uint8_t* cmd, size_t len);
```

### مثال C

```c
#include <stdio.h>
#include "venom_memory_rs.h"

int main() {
    // الاتصال
    VenomShellHandle* shell = venom_shell_connect("my_channel");
    if (!shell) {
        printf("❌ فشل الاتصال\n");
        return 1;
    }
    
    printf("✅ متصل! ID: %u\n", venom_shell_id(shell));
    
    // إرسال أمر
    venom_shell_send_command(shell, (uint8_t*)"PING", 4);
    
    // قراءة
    uint8_t buf[1024];
    size_t len = venom_shell_read_data(shell, buf, sizeof(buf));
    printf("📥 استلمت %zu بايت\n", len);
    
    venom_shell_destroy(shell);
    return 0;
}
```

---

## 📱 الاستخدام من Flutter/Dart

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

### استخدام في Flutter Widget

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
          child: Text('معايرة'),
        ),
      ],
    );
  }
}
```

---

## 📝 API المرجعية

### DaemonChannel

| الدالة | الوصف |
|--------|-------|
| `create(name, config)` | إنشاء قناة جديدة |
| `write_data(bytes)` | كتابة بيانات (يقرأها جميع الشللز) |
| `try_recv_command(buf)` | استقبال أمر (غير محجوب) |
| `as_ptr()` | مؤشر خام للذاكرة |

### ShellChannel

| الدالة | الوصف |
|--------|-------|
| `connect(name)` | الاتصال بقناة موجودة |
| `read_data(buf)` | قراءة بيانات من الخادم |
| `try_send_command(bytes)` | إرسال أمر للخادم |
| `client_id()` | معرف العميل الفريد |
| `as_ptr()` | مؤشر خام للذاكرة |

### ChannelConfig

| الحقل | النوع | الوصف |
|-------|------|-------|
| `data_size` | `usize` | حجم منطقة البيانات |
| `cmd_slots` | `usize` | عدد خانات الأوامر |
| `max_clients` | `usize` | أقصى عدد للعملاء |

---

## ⚠️ ملاحظات مهمة

### 1. تطابق الهياكل
```rust
// الخادم والعميل يجب أن يستخدموا نفس الهيكل!
#[repr(C)]  // إجباري للتوافق
struct MyData {
    field1: f32,  // نفس الترتيب
    field2: u32,  // نفس الأنواع
}
```

### 2. معالجة الأخطاء
```rust
// تحقق دائماً من نجاح الاتصال
let shell = match ShellChannel::connect("channel") {
    Ok(s) => s,
    Err(e) => {
        eprintln!("فشل الاتصال: {:?}", e);
        return;
    }
};
```

### 3. تنظيف الموارد
```rust
// الموارد تُحرر تلقائياً في Rust (Drop)
// في C يجب استدعاء destroy:
venom_shell_destroy(shell);
```

### 4. الأمان متعدد الخيوط
```rust
// VenomMemory آمنة للخيوط (Thread-safe)
// يمكن مشاركة Shell بين خيوط متعددة
let shell = Arc::new(shell);
```

---

## 🚀 أفضل الممارسات

1. **استخدم `#[repr(C)]`** لجميع الهياكل المشتركة
2. **تحقق من حجم البيانات** قبل القراءة
3. **لا تحجب الخادم** - استخدم `try_recv_command`
4. **اختر حجم مناسب** لـ `data_size` و `cmd_slots`
5. **أوقف الخادم** بأمان عند الإنهاء

---

## 📊 الأداء المتوقع

| المقياس | القيمة |
|---------|--------|
| عرض النطاق | ~40 GB/s |
| زمن الاستجابة | ~50 µs |
| syscalls | 0 (بعد الإنشاء) |

---

## 🔗 روابط مفيدة

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) - البنية التقنية
- [examples/system_daemon.rs](../examples/system_daemon.rs) - مثال كامل
- [examples/status_bar.rs](../examples/status_bar.rs) - مثال العميل
