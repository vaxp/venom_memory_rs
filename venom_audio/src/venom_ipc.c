/**
 * VenomMemory IPC Service for Audio Daemon
 * 
 * Replaces D-Bus with high-performance lock-free shared memory IPC.
 * The daemon writes audio state to shared memory, and handles commands
 * from clients via the MPSC queue.
*/

#include "audio.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <pthread.h>

// ═══════════════════════════════════════════════════════════════════════════
// 📦 VenomMemory Bindings
// ═══════════════════════════════════════════════════════════════════════════

typedef struct VenomDaemonHandle VenomDaemonHandle;

typedef struct {
    size_t data_size;
    size_t cmd_slots;
    size_t max_clients;
} VenomConfig;

extern VenomDaemonHandle* venom_daemon_create(const char* name, VenomConfig config);
extern void venom_daemon_destroy(VenomDaemonHandle* handle);
extern void venom_daemon_write_data(VenomDaemonHandle* handle, const uint8_t* data, size_t len);
extern size_t venom_daemon_try_recv_command(VenomDaemonHandle* handle, uint8_t* buf, size_t max_len, uint32_t* out_client_id);
extern uint8_t* venom_daemon_get_shm_ptr(VenomDaemonHandle* handle);

// ═══════════════════════════════════════════════════════════════════════════
// 📊 Shared Data Structures (what clients will read)
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
// 📨 Command Protocol (sent by clients via MPSC queue)
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
    CMD_REFRESH,  // Force state refresh
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
// 🔌 IPC State
// ═══════════════════════════════════════════════════════════════════════════

static VenomDaemonHandle *venom_handle = NULL;
static VenomAudioState shared_state = {0};
static pthread_mutex_t state_mutex = PTHREAD_MUTEX_INITIALIZER;
static uint64_t update_counter = 0;

// ═══════════════════════════════════════════════════════════════════════════
// 📤 State Publishing
// ═══════════════════════════════════════════════════════════════════════════

static void venom_update_devices(void) {
    pthread_mutex_lock(&state_mutex);
    
    // Update sinks
    GList *sinks = audio_get_sinks();
    shared_state.sink_count = 0;
    for (GList *l = sinks; l && shared_state.sink_count < MAX_DEVICES; l = l->next) {
        AudioDevice *d = l->data;
        VenomAudioDevice *vd = &shared_state.sinks[shared_state.sink_count];
        strncpy(vd->name, d->name ? d->name : "", MAX_DEVICE_NAME - 1);
        strncpy(vd->description, d->description ? d->description : "", MAX_DEVICE_NAME - 1);
        vd->volume = d->volume;
        vd->muted = d->muted;
        vd->is_default = d->is_default;
        shared_state.sink_count++;
        audio_device_free(d);
    }
    g_list_free(sinks);
    
    // Update sources
    GList *sources = audio_get_sources();
    shared_state.source_count = 0;
    for (GList *l = sources; l && shared_state.source_count < MAX_DEVICES; l = l->next) {
        AudioDevice *d = l->data;
        VenomAudioDevice *vd = &shared_state.sources[shared_state.source_count];
        strncpy(vd->name, d->name ? d->name : "", MAX_DEVICE_NAME - 1);
        strncpy(vd->description, d->description ? d->description : "", MAX_DEVICE_NAME - 1);
        vd->volume = d->volume;
        vd->muted = d->muted;
        vd->is_default = d->is_default;
        shared_state.source_count++;
        audio_device_free(d);
    }
    g_list_free(sources);
    
    pthread_mutex_unlock(&state_mutex);
}

static void venom_update_apps(void) {
    pthread_mutex_lock(&state_mutex);
    
    GList *apps = audio_get_app_streams();
    shared_state.app_count = 0;
    for (GList *l = apps; l && shared_state.app_count < MAX_APP_STREAMS; l = l->next) {
        AppStream *a = l->data;
        VenomAppStream *va = &shared_state.apps[shared_state.app_count];
        va->index = a->index;
        strncpy(va->name, a->name ? a->name : "", 63);
        strncpy(va->icon, a->icon ? a->icon : "", 63);
        va->volume = a->volume;
        va->muted = a->muted;
        shared_state.app_count++;
        audio_app_stream_free(a);
    }
    g_list_free(apps);
    
    pthread_mutex_unlock(&state_mutex);
}

void venom_publish_state(void) {
    if (!venom_handle) return;
    
    pthread_mutex_lock(&state_mutex);
    
    // Update basic state
    shared_state.magic = VENOM_AUDIO_MAGIC;
    shared_state.version = 1;
    shared_state.volume = audio_get_volume();
    shared_state.mic_volume = audio_get_mic_volume();
    shared_state.muted = audio_get_muted();
    shared_state.mic_muted = audio_get_mic_muted();
    shared_state.overamplification = audio_get_overamplification();
    shared_state.max_volume = shared_state.overamplification ? 150 : 100;
    
    // Copy default device names
    if (audio_state.default_sink) {
        strncpy(shared_state.default_sink, audio_state.default_sink, MAX_DEVICE_NAME - 1);
    }
    if (audio_state.default_source) {
        strncpy(shared_state.default_source, audio_state.default_source, MAX_DEVICE_NAME - 1);
    }
    
    // Update counter and timestamp
    shared_state.update_counter = ++update_counter;
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    shared_state.timestamp_ns = (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
    
    // Write to shared memory
    venom_daemon_write_data(venom_handle, (const uint8_t*)&shared_state, sizeof(shared_state));
    
    pthread_mutex_unlock(&state_mutex);
}

// ═══════════════════════════════════════════════════════════════════════════
// 📡 Callbacks from audio.c
// ═══════════════════════════════════════════════════════════════════════════

void venom_on_volume_changed(gint volume) {
    (void)volume;
    audio_state.pending_updates |= UPDATE_PUBLISH;
}

void venom_on_mute_changed(gboolean muted) {
    (void)muted;
    audio_state.pending_updates |= UPDATE_PUBLISH;
}

void venom_on_devices_changed(void) {
    audio_state.pending_updates |= (UPDATE_DEVICES | UPDATE_PUBLISH);
}

void venom_on_apps_changed(void) {
    audio_state.pending_updates |= (UPDATE_APPS | UPDATE_PUBLISH);
}

void venom_ipc_sync(void) {
    if (audio_state.pending_updates & UPDATE_DEVICES) {
        venom_update_devices();
    }
    if (audio_state.pending_updates & UPDATE_APPS) {
        venom_update_apps();
    }
    if (audio_state.pending_updates & UPDATE_PUBLISH) {
        venom_publish_state();
    }
    audio_state.pending_updates = 0;
}

// ═══════════════════════════════════════════════════════════════════════════
// 📨 Command Processing (TODO: implement command receiving)
// ═══════════════════════════════════════════════════════════════════════════

void venom_process_command(const uint8_t *cmd_data, size_t len) {
    // Minimum size: 4 bytes (cmd + padding) + 4 bytes (int32 data)
    if (len < 8) {
        printf("⚠️ Command too small: %zu < 8\n", len);
        return;
    }
    
    const VenomAudioCommand *cmd = (const VenomAudioCommand*)cmd_data;
    printf("🔧 Processing cmd=%d, volume=%d, muted=%d\n", cmd->cmd, cmd->data.volume, cmd->data.muted);
    
    switch (cmd->cmd) {
        case CMD_SET_VOLUME:
            printf("🔊 Setting volume to %d\n", cmd->data.volume);
            audio_set_volume(cmd->data.volume);
            break;
        case CMD_SET_MUTED:
            printf("🔇 Setting muted to %d\n", cmd->data.muted);
            audio_set_muted(cmd->data.muted);
            break;
        case CMD_SET_MIC_VOLUME:
            printf("🎤 Setting mic volume to %d\n", cmd->data.volume);
            audio_set_mic_volume(cmd->data.volume);
            break;
        case CMD_SET_MIC_MUTED:
            printf("🎤🔇 Setting mic muted to %d\n", cmd->data.muted);
            audio_set_mic_muted(cmd->data.muted);
            break;
        case CMD_SET_DEFAULT_SINK:
            printf("🔈 Setting default sink to %s\n", cmd->data.device.name);
            audio_set_default_sink(cmd->data.device.name);
            break;
        case CMD_SET_DEFAULT_SOURCE:
            printf("🎤 Setting default source to %s\n", cmd->data.device.name);
            audio_set_default_source(cmd->data.device.name);
            break;
        case CMD_SET_SINK_VOLUME:
            audio_set_sink_volume(cmd->data.device_vol.name, cmd->data.device_vol.volume);
            break;
        case CMD_SET_SOURCE_VOLUME:
            audio_set_source_volume(cmd->data.device_vol.name, cmd->data.device_vol.volume);
            break;
        case CMD_SET_APP_VOLUME:
            audio_set_app_volume(cmd->data.app_vol.index, cmd->data.app_vol.volume);
            venom_update_apps();
            venom_publish_state();
            break;
        case CMD_SET_APP_MUTED:
            audio_set_app_muted(cmd->data.app_mute.index, cmd->data.app_mute.muted);
            venom_update_apps();
            venom_publish_state();
            break;
        case CMD_MOVE_APP_TO_SINK:
            audio_move_app_to_sink(cmd->data.app_sink.index, cmd->data.app_sink.sink);
            break;
        case CMD_SET_OVERAMPLIFICATION:
            audio_set_overamplification(cmd->data.enabled);
            break;
        case CMD_SET_PROFILE:
            audio_set_profile(cmd->data.profile.card, cmd->data.profile.profile);
            break;
        case CMD_REFRESH:
            venom_update_devices();
            venom_update_apps();
            venom_publish_state();
            break;
        default:
            printf("⚠️ Unknown command: %d\n", cmd->cmd);
            break;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 🚀 Initialization
// ═══════════════════════════════════════════════════════════════════════════

gboolean venom_ipc_init(void) {
    printf("🔗 Initializing VenomMemory IPC...\n");
    
    VenomConfig config = {
        .data_size = sizeof(VenomAudioState) + 256,  // State + padding
        .cmd_slots = 32,
        .max_clients = 16
    };
    
    venom_handle = venom_daemon_create("venom_audio", config);
    if (!venom_handle) {
        printf("❌ Failed to create VenomMemory channel\n");
        return FALSE;
    }
    
    printf("✅ VenomMemory channel created: venom_audio\n");
    printf("📊 Shared state size: %zu bytes\n", sizeof(VenomAudioState));
    
    // Initial state update
    memset(&shared_state, 0, sizeof(shared_state));
    venom_update_devices();
    venom_update_apps();
    venom_publish_state();
    
    return TRUE;
}

void venom_ipc_cleanup(void) {
    if (venom_handle) {
        venom_daemon_destroy(venom_handle);
        venom_handle = NULL;
    }
    printf("🔗 VenomMemory IPC cleaned up\n");
}

VenomDaemonHandle* venom_ipc_get_handle(void) {
    return venom_handle;
}

void venom_poll_commands(void) {
    if (!venom_handle) return;
    
    uint8_t cmd_buf[4096];
    uint32_t client_id = 0;
    
    // Process all pending commands
    while (1) {
        size_t len = venom_daemon_try_recv_command(venom_handle, cmd_buf, sizeof(cmd_buf), &client_id);
        if (len == 0) break;
        
        printf("📥 Command from client %u (%zu bytes)\n", client_id, len);
        venom_process_command(cmd_buf, len);
    }
}

