#ifndef VENOM_IPC_H
#define VENOM_IPC_H

#include <glib.h>
#include <stdint.h>

// ═══════════════════════════════════════════════════════════════════════════
// 📊 Shared Data Structures
// ═══════════════════════════════════════════════════════════════════════════

#define VENOM_AUDIO_MAGIC 0x564E4155  // "VNAU"
#define MAX_DEVICE_NAME 128
#define MAX_DEVICES 16
#define MAX_APP_STREAMS 32

// Device info structure
typedef struct {
    char name[MAX_DEVICE_NAME];
    char description[MAX_DEVICE_NAME];
    int32_t volume;
    uint8_t muted;
    uint8_t is_default;
    uint8_t _pad[2];
} VenomAudioDevice;

// Application stream info
typedef struct {
    uint32_t index;
    char name[64];
    char icon[64];
    int32_t volume;
    uint8_t muted;
    uint8_t _pad[3];
} VenomAppStream;

// Main audio state structure (written by daemon, read by clients)
typedef struct {
    uint32_t magic;
    uint32_t version;
    
    // Master volume
    int32_t volume;
    int32_t mic_volume;
    uint8_t muted;
    uint8_t mic_muted;
    uint8_t overamplification;
    uint8_t _pad1;
    int32_t max_volume;
    
    // Default devices
    char default_sink[MAX_DEVICE_NAME];
    char default_source[MAX_DEVICE_NAME];
    
    // Output devices (sinks)
    uint32_t sink_count;
    VenomAudioDevice sinks[MAX_DEVICES];
    
    // Input devices (sources)
    uint32_t source_count;
    VenomAudioDevice sources[MAX_DEVICES];
    
    // Application streams
    uint32_t app_count;
    VenomAppStream apps[MAX_APP_STREAMS];
    
    // Timestamp for change detection
    uint64_t update_counter;
    uint64_t timestamp_ns;
} VenomAudioState;

// ═══════════════════════════════════════════════════════════════════════════
// 📨 Command Protocol
// ═══════════════════════════════════════════════════════════════════════════

typedef enum {
    CMD_SET_VOLUME = 1,
    CMD_SET_MUTED,
    CMD_SET_MIC_VOLUME,
    CMD_SET_MIC_MUTED,
    CMD_SET_DEFAULT_SINK,
    CMD_SET_DEFAULT_SOURCE,
    CMD_SET_SINK_VOLUME,
    CMD_SET_SOURCE_VOLUME,
    CMD_SET_APP_VOLUME,
    CMD_SET_APP_MUTED,
    CMD_MOVE_APP_TO_SINK,
    CMD_SET_OVERAMPLIFICATION,
    CMD_SET_PROFILE,
    CMD_REFRESH,
} VenomAudioCmd;

typedef struct {
    uint8_t cmd;
    uint8_t _pad[3];
    union {
        int32_t volume;
        uint8_t muted;
        uint8_t enabled;
        struct { char name[MAX_DEVICE_NAME]; } device;
        struct { char name[MAX_DEVICE_NAME]; int32_t volume; } device_vol;
        struct { uint32_t index; int32_t volume; } app_vol;
        struct { uint32_t index; uint8_t muted; } app_mute;
        struct { uint32_t index; char sink[MAX_DEVICE_NAME]; } app_sink;
        struct { char card[MAX_DEVICE_NAME]; char profile[MAX_DEVICE_NAME]; } profile;
    } data;
} VenomAudioCommand;

// ═══════════════════════════════════════════════════════════════════════════
// 🔧 Functions
// ═══════════════════════════════════════════════════════════════════════════

// Initialization
gboolean venom_ipc_init(void);
void venom_ipc_cleanup(void);

// State publishing
void venom_publish_state(void);

// Callbacks (to be set on audio_state)
void venom_on_volume_changed(gint volume);
void venom_on_mute_changed(gboolean muted);
void venom_on_devices_changed(void);
void venom_on_apps_changed(void);

// Command processing
void venom_process_command(const uint8_t *cmd_data, size_t len);
void venom_poll_commands(void);
void venom_ipc_sync(void);

#endif // VENOM_IPC_H
