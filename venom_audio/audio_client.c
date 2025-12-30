/**
 * VenomMemory Audio Client - GTK3 GUI
 * 
 * Reads audio state from venom_audio daemon via shared memory
 * and sends commands via MPSC queue.
 * 
 * Build: gcc -O2 `pkg-config --cflags gtk+-3.0` audio_client.c -o audio_client \
 *        `pkg-config --libs gtk+-3.0` -L../target/release -lvenom_memory -Wl,-rpath,../target/release
 */

#include <gtk/gtk.h>
#include <stdint.h>
#include <string.h>
#include <time.h>

// ═══════════════════════════════════════════════════════════════════════════
// 📦 VenomMemory Bindings
// ═══════════════════════════════════════════════════════════════════════════

typedef struct VenomShellHandle VenomShellHandle;

extern VenomShellHandle* venom_shell_connect(const char* name);
extern void venom_shell_destroy(VenomShellHandle* handle);
extern size_t venom_shell_read_data(VenomShellHandle* handle, uint8_t* buf, size_t max_len);
extern uint32_t venom_shell_id(VenomShellHandle* handle);
extern int venom_shell_send_command(VenomShellHandle* handle, const uint8_t* cmd, size_t len);

// ═══════════════════════════════════════════════════════════════════════════
// 📊 Shared Data Structures (must match venom_ipc.h)
// ═══════════════════════════════════════════════════════════════════════════

#define VENOM_AUDIO_MAGIC 0x564E4155
#define MAX_DEVICE_NAME 128
#define MAX_DEVICES 16
#define MAX_APP_STREAMS 32

typedef struct {
    char name[MAX_DEVICE_NAME];
    char description[MAX_DEVICE_NAME];
    int32_t volume;
    uint8_t muted;
    uint8_t is_default;
    uint8_t _pad[2];
} VenomAudioDevice;

typedef struct {
    uint32_t index;
    char name[64];
    char icon[64];
    int32_t volume;
    uint8_t muted;
    char sink[MAX_DEVICE_NAME];
    uint8_t _pad[3];
} VenomAppStream;

typedef struct {
    uint32_t magic;
    uint32_t version;
    int32_t volume;
    int32_t mic_volume;
    uint8_t muted;
    uint8_t mic_muted;
    uint8_t overamplification;
    uint8_t _pad1;
    int32_t max_volume;
    char default_sink[MAX_DEVICE_NAME];
    char default_source[MAX_DEVICE_NAME];
    uint32_t sink_count;
    VenomAudioDevice sinks[MAX_DEVICES];
    uint32_t source_count;
    VenomAudioDevice sources[MAX_DEVICES];
    uint32_t app_count;
    VenomAppStream apps[MAX_APP_STREAMS];
    uint64_t update_counter;
    uint64_t timestamp_ns;
} VenomAudioState;

// Command protocol
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
// 🌐 Global State
// ═══════════════════════════════════════════════════════════════════════════

static VenomShellHandle* g_shell = NULL;
static VenomAudioState g_state = {0};
static uint64_t g_last_counter = 0;
static int g_frame = 0;
static struct timespec g_last_read = {0};

// GTK Widgets
static GtkWidget* g_volume_scale = NULL;
static GtkWidget* g_mute_btn = NULL;
static GtkWidget* g_mic_scale = NULL;
static GtkWidget* g_mic_mute_btn = NULL;
static GtkWidget* g_status_label = NULL;
static GtkWidget* g_latency_label = NULL;
static GtkWidget* g_sinks_combo = NULL;
static GtkWidget* g_apps_box = NULL;
static GtkWidget* g_update_label = NULL;

static GtkWidget* g_sources_combo = NULL;
static GtkWidget* g_overamp_check = NULL;

static gboolean g_updating_ui = FALSE;
static struct timespec g_last_cmd_sent = {0};

static float timespec_diff_ms(struct timespec* start, struct timespec* end) {
    return (end->tv_sec - start->tv_sec) * 1000.0f + (end->tv_nsec - start->tv_nsec) / 1000000.0f;
}

// ═══════════════════════════════════════════════════════════════════════════
// 📨 Command Sending
// ═══════════════════════════════════════════════════════════════════════════

static void send_volume_command(int volume) {
    if (!g_shell) return;
    clock_gettime(CLOCK_MONOTONIC, &g_last_cmd_sent);
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_VOLUME;
    cmd.data.volume = volume;
    int result = venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent volume command: %d (result=%d, size=%zu)\n", volume, result, sizeof(cmd));
}

static void send_mute_command(gboolean muted) {
    if (!g_shell) return;
    clock_gettime(CLOCK_MONOTONIC, &g_last_cmd_sent);
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_MUTED;
    cmd.data.muted = muted ? 1 : 0;
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
}

static void send_mic_volume_command(int volume) {
    if (!g_shell) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_MIC_VOLUME;
    cmd.data.volume = volume;
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
}

static void send_mic_mute_command(gboolean muted) {
    if (!g_shell) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_MIC_MUTED;
    cmd.data.muted = muted ? 1 : 0;
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
}

static void send_default_sink_command(const char* name) {
    if (!g_shell || !name) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_DEFAULT_SINK;
    strncpy(cmd.data.device.name, name, MAX_DEVICE_NAME - 1);
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent default sink command: %s\n", name);
}

static void send_default_source_command(const char* name) {
    if (!g_shell || !name) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_DEFAULT_SOURCE;
    strncpy(cmd.data.device.name, name, MAX_DEVICE_NAME - 1);
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent default source command: %s\n", name);
}

static void send_overamp_command(gboolean enabled) {
    if (!g_shell) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_OVERAMPLIFICATION;
    cmd.data.enabled = enabled;
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent overamplification command: %d\n", enabled);
}

static void send_move_app_command(uint32_t index, const char* sink_name) {
    if (!g_shell || !sink_name) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_MOVE_APP_TO_SINK;
    cmd.data.app_sink.index = index;
    strncpy(cmd.data.app_sink.sink, sink_name, MAX_DEVICE_NAME - 1);
    venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent move app command: %u -> %s\n", index, sink_name);
}

static void send_app_volume_command(uint32_t index, int volume) {
    if (!g_shell) return;
    clock_gettime(CLOCK_MONOTONIC, &g_last_cmd_sent);
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_APP_VOLUME;
    cmd.data.app_vol.index = index;
    cmd.data.app_vol.volume = volume;
    int result = venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent APP volume command: AppIndex=%u, Vol=%d (result=%d)\n", index, volume, result);
}

static void send_app_mute_command(uint32_t index, gboolean muted) {
    if (!g_shell) return;
    clock_gettime(CLOCK_MONOTONIC, &g_last_cmd_sent);
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_SET_APP_MUTED;
    cmd.data.app_mute.index = index;
    cmd.data.app_mute.muted = muted ? 1 : 0;
    int result = venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent APP mute command: AppIndex=%u, Muted=%d (result=%d)\n", index, muted, result);
}

static void send_refresh_command(void) {
    if (!g_shell) return;
    VenomAudioCommand cmd = {0};
    cmd.cmd = CMD_REFRESH;
    int result = venom_shell_send_command(g_shell, (uint8_t*)&cmd, sizeof(cmd));
    g_print("📤 Sent REFRESH command (result=%d)\n", result);
}

static void on_refresh_clicked(GtkButton* btn, gpointer data) {
    (void)btn; (void)data;
    send_refresh_command();
}

// ═══════════════════════════════════════════════════════════════════════════
// 🎛️ UI Callbacks
// ═══════════════════════════════════════════════════════════════════════════

static void on_volume_changed(GtkRange* range, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    int vol = (int)gtk_range_get_value(range);
    send_volume_command(vol);
}

static void on_mute_toggled(GtkToggleButton* btn, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    send_mute_command(gtk_toggle_button_get_active(btn));
}

static void on_mic_volume_changed(GtkRange* range, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    int vol = (int)gtk_range_get_value(range);
    send_mic_volume_command(vol);
}

static void on_mic_mute_toggled(GtkToggleButton* btn, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    send_mic_mute_command(gtk_toggle_button_get_active(btn));
}

static void on_sink_changed(GtkComboBox* combo, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    
    int index = gtk_combo_box_get_active(combo);
    if (index >= 0 && (uint32_t)index < g_state.sink_count) {
        send_default_sink_command(g_state.sinks[index].name);
    }
}

static void on_source_changed(GtkComboBox* combo, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    
    int index = gtk_combo_box_get_active(combo);
    if (index >= 0 && (uint32_t)index < g_state.source_count) {
        send_default_source_command(g_state.sources[index].name);
    }
}

static void on_overamp_toggled(GtkCheckButton* btn, gpointer data) {
    (void)data;
    if (g_updating_ui) return;
    send_overamp_command(gtk_toggle_button_get_active(GTK_TOGGLE_BUTTON(btn)));
}

static void on_app_sink_changed(GtkComboBox* combo, gpointer data) {
    uint32_t index = GPOINTER_TO_UINT(data);
    if (g_updating_ui) return;
    
    // Find device name from combo text
    // Note: This is simple but slightly fragile if device names have odd characters
    // Better would be to store ID in model, but for C/GTK basic client this is fine
    const char* active_id = gtk_combo_box_get_active_id(combo);
    if (active_id) {
        send_move_app_command(index, active_id);
    }
}

static void on_app_volume_changed(GtkRange* range, gpointer data) {
    uint32_t index = GPOINTER_TO_UINT(data);
    if (g_updating_ui) return;
    int vol = (int)gtk_range_get_value(range);
    g_print("🖱️ UI: App %u volume slider moved to %d\n", index, vol);
    send_app_volume_command(index, vol);
}

static void on_app_mute_toggled(GtkToggleButton* btn, gpointer data) {
    uint32_t index = GPOINTER_TO_UINT(data);
    if (g_updating_ui) return;
    gboolean muted = gtk_toggle_button_get_active(btn);
    g_print("🖱️ UI: App %u mute toggled to %d\n", index, muted);
    send_app_mute_command(index, muted);
}

static void refresh_app_list(void) {
    static uint32_t last_app_count = 999;
    static char last_app_names[MAX_APP_STREAMS][64];
    
    gboolean structure_changed = (g_state.app_count != last_app_count);
    if (!structure_changed) {
        for (uint32_t i = 0; i < g_state.app_count; i++) {
            if (strcmp(g_state.apps[i].name, last_app_names[i]) != 0) {
                structure_changed = TRUE;
                break;
            }
        }
    }

    if (structure_changed) {
        // Full rebuild
        GList *children = gtk_container_get_children(GTK_CONTAINER(g_apps_box));
        for (GList *iter = children; iter != NULL; iter = g_list_next(iter)) {
            gtk_widget_destroy(GTK_WIDGET(iter->data));
        }
        g_list_free(children);

        if (g_state.app_count == 0) {
            GtkWidget* empty = gtk_label_new("No active application streams");
            gtk_widget_set_sensitive(empty, FALSE);
            gtk_box_pack_start(GTK_BOX(g_apps_box), empty, FALSE, FALSE, 10);
            gtk_widget_show(empty);
        } else {
    for (uint32_t i = 0; i < g_state.app_count && i < MAX_APP_STREAMS; i++) {
        GtkWidget* row = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
        char label_text[128];
        snprintf(label_text, sizeof(label_text), "%s %s", 
            g_state.apps[i].icon[0] ? g_state.apps[i].icon : "📱",
            g_state.apps[i].name);
        strncpy(last_app_names[i], g_state.apps[i].name, 63);
        
        g_print("DEBUG: Rebuild App %u: %s, Vol=%d, Muted=%d, ID=%u\n", 
            i, g_state.apps[i].name, g_state.apps[i].volume, g_state.apps[i].muted, g_state.apps[i].index);
            
        GtkWidget* label = gtk_label_new(label_text);
        gtk_widget_set_size_request(label, 150, -1);
        gtk_label_set_xalign(GTK_LABEL(label), 0.0);
        
        GtkWidget* scale = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 100, 1);
        gtk_scale_set_value_pos(GTK_SCALE(scale), GTK_POS_RIGHT);
        gtk_range_set_value(GTK_RANGE(scale), g_state.apps[i].volume);
        g_object_set_data(G_OBJECT(row), "scale", scale);
        
        GtkWidget* mute = gtk_toggle_button_new_with_label("🔇");
        gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(mute), g_state.apps[i].muted);
        g_object_set_data(G_OBJECT(row), "mute", mute);
        
        gtk_box_pack_start(GTK_BOX(row), label, FALSE, FALSE, 0);
        gtk_box_pack_start(GTK_BOX(row), scale, TRUE, TRUE, 0);
        gtk_box_pack_start(GTK_BOX(row), mute, FALSE, FALSE, 0);
        
        // App Sink Combo
        GtkWidget* combo = gtk_combo_box_text_new();
        g_object_set_data(G_OBJECT(row), "combo", combo);
        for (uint32_t s = 0; s < g_state.sink_count; s++) {
            gtk_combo_box_text_append(GTK_COMBO_BOX_TEXT(combo), 
                g_state.sinks[s].name, g_state.sinks[s].description);
        }
        if (g_state.apps[i].sink[0]) {
            gtk_combo_box_set_active_id(GTK_COMBO_BOX(combo), g_state.apps[i].sink);
        }
        gtk_box_pack_start(GTK_BOX(row), combo, FALSE, FALSE, 0);
        
        g_signal_connect(scale, "value-changed", G_CALLBACK(on_app_volume_changed), GUINT_TO_POINTER(g_state.apps[i].index));
        g_signal_connect(mute, "toggled", G_CALLBACK(on_app_mute_toggled), GUINT_TO_POINTER(g_state.apps[i].index));
        g_signal_connect(combo, "changed", G_CALLBACK(on_app_sink_changed), GUINT_TO_POINTER(g_state.apps[i].index));
        
        gtk_box_pack_start(GTK_BOX(g_apps_box), row, FALSE, FALSE, 0);
    }
        }
        gtk_widget_show_all(g_apps_box);
        last_app_count = g_state.app_count;
    } else {
        // Just update values of existing widgets
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    if (timespec_diff_ms(&g_last_cmd_sent, &now) < 500.0f) return;

    g_updating_ui = TRUE;
    GList *children = gtk_container_get_children(GTK_CONTAINER(g_apps_box));
    uint32_t i = 0;
    for (GList *iter = children; iter != NULL && i < g_state.app_count; iter = g_list_next(iter), i++) {
        GtkWidget* row = GTK_WIDGET(iter->data);
        GtkWidget* scale = GTK_WIDGET(g_object_get_data(G_OBJECT(row), "scale"));
        GtkWidget* mute = GTK_WIDGET(g_object_get_data(G_OBJECT(row), "mute"));
        
        if (scale) {
            int cur_val = (int)gtk_range_get_value(GTK_RANGE(scale));
            if (cur_val != g_state.apps[i].volume) {
                g_print("DEBUG: Updating App %u slider: %d -> %d\n", i, cur_val, g_state.apps[i].volume);
                gtk_range_set_value(GTK_RANGE(scale), g_state.apps[i].volume);
            }
        }
        if (mute) {
            gboolean cur_muted = gtk_toggle_button_get_active(GTK_TOGGLE_BUTTON(mute));
            if (cur_muted != g_state.apps[i].muted) {
                g_print("DEBUG: Updating App %u mute: %d -> %d\n", i, cur_muted, g_state.apps[i].muted);
                gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(mute), g_state.apps[i].muted);
            }
        }
        
        GtkWidget* combo = GTK_WIDGET(g_object_get_data(G_OBJECT(row), "combo"));
        if (combo && g_state.apps[i].sink[0]) {
             const char* active_id = gtk_combo_box_get_active_id(GTK_COMBO_BOX(combo));
             if (!active_id || strcmp(active_id, g_state.apps[i].sink) != 0) {
                 gtk_combo_box_set_active_id(GTK_COMBO_BOX(combo), g_state.apps[i].sink);
             }
        }
    }
    g_list_free(children);
    g_updating_ui = FALSE;
}
}

// ═══════════════════════════════════════════════════════════════════════════
// 🔄 Update Loop
// ═══════════════════════════════════════════════════════════════════════════

static gboolean update_ui(gpointer data) {
    (void)data;
    if (!g_shell) return G_SOURCE_CONTINUE;
    
    struct timespec before, after;
    clock_gettime(CLOCK_MONOTONIC, &before);
    
    // Read state from shared memory
    uint8_t buf[sizeof(VenomAudioState) + 64];
    size_t len = venom_shell_read_data(g_shell, buf, sizeof(buf));
    
    clock_gettime(CLOCK_MONOTONIC, &after);
    
    // Calculate read latency
    long latency_ns = (after.tv_sec - before.tv_sec) * 1000000000L +
                      (after.tv_nsec - before.tv_nsec);
    
    if (len >= sizeof(VenomAudioState)) {
        memcpy(&g_state, buf, sizeof(VenomAudioState));
        
        if (g_state.magic == VENOM_AUDIO_MAGIC) {
            g_frame++;
            
            // Check if state changed
            if (g_state.update_counter != g_last_counter) {
                g_last_counter = g_state.update_counter;
                
                g_updating_ui = TRUE;
                
                // Update volume slider
                gtk_range_set_value(GTK_RANGE(g_volume_scale), g_state.volume);
                
                // Update mute button
                gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(g_mute_btn), g_state.muted);
                
                // Update mic slider
                gtk_range_set_value(GTK_RANGE(g_mic_scale), g_state.mic_volume);
                
                // Update mic mute button
                gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(g_mic_mute_btn), g_state.mic_muted);
                
                // Update sinks combo
                gtk_combo_box_text_remove_all(GTK_COMBO_BOX_TEXT(g_sinks_combo));
                for (uint32_t i = 0; i < g_state.sink_count && i < MAX_DEVICES; i++) {
                    char label[256];
                    snprintf(label, sizeof(label), "%s%s", 
                        g_state.sinks[i].description,
                        g_state.sinks[i].is_default ? " ✓" : "");
                    gtk_combo_box_text_append_text(GTK_COMBO_BOX_TEXT(g_sinks_combo), label);
                    if (g_state.sinks[i].is_default) {
                        gtk_combo_box_set_active(GTK_COMBO_BOX(g_sinks_combo), i);
                    }
                }
                
                // Update sources combo
                gtk_combo_box_text_remove_all(GTK_COMBO_BOX_TEXT(g_sources_combo));
                for (uint32_t i = 0; i < g_state.source_count && i < MAX_DEVICES; i++) {
                    char label[256];
                    snprintf(label, sizeof(label), "%s%s", 
                        g_state.sources[i].description,
                        g_state.sources[i].is_default ? " ✓" : "");
                    gtk_combo_box_text_append_text(GTK_COMBO_BOX_TEXT(g_sources_combo), label);
                    if (g_state.sources[i].is_default) {
                        gtk_combo_box_set_active(GTK_COMBO_BOX(g_sources_combo), i);
                    }
                }
                
                // Update overamp
                gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(g_overamp_check), g_state.overamplification);

                // Update Apps
                refresh_app_list();
                
                g_updating_ui = FALSE;
            }
            
            // Update status
            char status[256];
            snprintf(status, sizeof(status), 
                "🔊 Vol: %d%% %s | 🎤 Mic: %d%% %s | Sinks: %u | Apps: %u",
                g_state.volume, g_state.muted ? "🔇" : "",
                g_state.mic_volume, g_state.mic_muted ? "🔇" : "",
                g_state.sink_count, g_state.app_count);
            gtk_label_set_text(GTK_LABEL(g_status_label), status);
            
            // Update latency
            char lat_text[128];
            snprintf(lat_text, sizeof(lat_text), 
                "📊 Frame: %d | Read: %.2f µs | Updates: %lu",
                g_frame, latency_ns / 1000.0, (unsigned long)g_state.update_counter);
            gtk_label_set_text(GTK_LABEL(g_latency_label), lat_text);
        } else {
            gtk_label_set_text(GTK_LABEL(g_status_label), "❌ Invalid magic number");
        }
    } else {
        gtk_label_set_text(GTK_LABEL(g_status_label), "⏳ Waiting for daemon...");
    }
    
    return G_SOURCE_CONTINUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🚀 Main
// ═══════════════════════════════════════════════════════════════════════════

int main(int argc, char** argv) {
    gtk_init(&argc, &argv);
    
    // Connect to VenomMemory
    g_shell = venom_shell_connect("venom_audio");
    if (!g_shell) {
        g_print("❌ Cannot connect to venom_audio daemon!\n");
        g_print("   Run: cd venom_audio && make run\n");
    } else {
        g_print("✅ Connected! Client ID: %u\n", venom_shell_id(g_shell));
    }
    
    // Create window
    GtkWidget* window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "🔊 VenomMemory Audio Client");
    gtk_window_set_default_size(GTK_WINDOW(window), 500, 400);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    
    // Main container
    GtkWidget* main_box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 10);
    gtk_container_set_border_width(GTK_CONTAINER(main_box), 15);
    gtk_container_add(GTK_CONTAINER(window), main_box);
    
    // Header area (Title + Refresh)
    GtkWidget* header_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget* title = gtk_label_new(NULL);
    gtk_label_set_markup(GTK_LABEL(title), 
        "<span size='x-large' weight='bold'>🔊 Venom Audio</span>");
    
    GtkWidget* refresh_btn = gtk_button_new_with_label("🔄 Refresh");
    gtk_button_set_relief(GTK_BUTTON(refresh_btn), GTK_RELIEF_NONE);
    g_signal_connect(refresh_btn, "clicked", G_CALLBACK(on_refresh_clicked), NULL);
    
    gtk_box_pack_start(GTK_BOX(header_box), title, FALSE, FALSE, 0);
    gtk_box_pack_end(GTK_BOX(header_box), refresh_btn, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(main_box), header_box, FALSE, FALSE, 5);
    
    // Status label
    g_status_label = gtk_label_new("Connecting...");
    gtk_box_pack_start(GTK_BOX(main_box), g_status_label, FALSE, FALSE, 5);
    
    // Separator
    gtk_box_pack_start(GTK_BOX(main_box), gtk_separator_new(GTK_ORIENTATION_HORIZONTAL), FALSE, FALSE, 5);
    
    // Volume section
    GtkWidget* vol_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget* vol_label = gtk_label_new("🔊 Volume:");
    gtk_widget_set_size_request(vol_label, 100, -1);
    g_volume_scale = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 150, 1);
    gtk_scale_set_value_pos(GTK_SCALE(g_volume_scale), GTK_POS_RIGHT);
    g_mute_btn = gtk_toggle_button_new_with_label("🔇 Mute");
    
    gtk_box_pack_start(GTK_BOX(vol_box), vol_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(vol_box), g_volume_scale, TRUE, TRUE, 0);
    
    // Overamp check
    g_overamp_check = gtk_check_button_new_with_label(">100%");
    gtk_box_pack_start(GTK_BOX(vol_box), g_overamp_check, FALSE, FALSE, 0);
    g_signal_connect(g_overamp_check, "toggled", G_CALLBACK(on_overamp_toggled), NULL);
    
    gtk_box_pack_start(GTK_BOX(vol_box), g_mute_btn, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(main_box), vol_box, FALSE, FALSE, 5);
    
    g_signal_connect(g_volume_scale, "value-changed", G_CALLBACK(on_volume_changed), NULL);
    g_signal_connect(g_mute_btn, "toggled", G_CALLBACK(on_mute_toggled), NULL);
    
    // Mic section
    GtkWidget* mic_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget* mic_label = gtk_label_new("🎤 Mic:");
    gtk_widget_set_size_request(mic_label, 100, -1);
    g_mic_scale = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 100, 1);
    gtk_scale_set_value_pos(GTK_SCALE(g_mic_scale), GTK_POS_RIGHT);
    g_mic_mute_btn = gtk_toggle_button_new_with_label("🔇 Mute");
    
    gtk_box_pack_start(GTK_BOX(mic_box), mic_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(mic_box), g_mic_scale, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(mic_box), g_mic_mute_btn, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(main_box), mic_box, FALSE, FALSE, 5);
    
    g_signal_connect(g_mic_scale, "value-changed", G_CALLBACK(on_mic_volume_changed), NULL);
    g_signal_connect(g_mic_mute_btn, "toggled", G_CALLBACK(on_mic_mute_toggled), NULL);
    
    // Separator
    gtk_box_pack_start(GTK_BOX(main_box), gtk_separator_new(GTK_ORIENTATION_HORIZONTAL), FALSE, FALSE, 5);
    
    // Output devices
    GtkWidget* sink_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget* sink_label = gtk_label_new("🔈 Output:");
    gtk_widget_set_size_request(sink_label, 100, -1);
    g_sinks_combo = gtk_combo_box_text_new();
    
    gtk_box_pack_start(GTK_BOX(sink_box), sink_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(sink_box), g_sinks_combo, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(main_box), sink_box, FALSE, FALSE, 5);
    
    g_signal_connect(g_sinks_combo, "changed", G_CALLBACK(on_sink_changed), NULL);
    
    // Input devices
    GtkWidget* src_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget* src_label = gtk_label_new("🎤 Input:");
    gtk_widget_set_size_request(src_label, 100, -1);
    g_sources_combo = gtk_combo_box_text_new();
    
    gtk_box_pack_start(GTK_BOX(src_box), src_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(src_box), g_sources_combo, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(main_box), src_box, FALSE, FALSE, 5);
    
    g_signal_connect(g_sources_combo, "changed", G_CALLBACK(on_source_changed), NULL);
    
    // Apps section title
    GtkWidget* app_title = gtk_label_new(NULL);
    gtk_label_set_markup(GTK_LABEL(app_title), "<b>📱 Application Streams</b>");
    gtk_widget_set_halign(app_title, GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(main_box), app_title, FALSE, FALSE, 10);
    
    // Apps scrollable area
    GtkWidget* scroll = gtk_scrolled_window_new(NULL, NULL);
    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scroll), GTK_POLICY_NEVER, GTK_POLICY_AUTOMATIC);
    gtk_widget_set_size_request(scroll, -1, 150);
    
    g_apps_box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 5);
    gtk_container_add(GTK_CONTAINER(scroll), g_apps_box);
    gtk_box_pack_start(GTK_BOX(main_box), scroll, TRUE, TRUE, 0);
    
    // Latency label
    g_latency_label = gtk_label_new("📊 Waiting...");
    gtk_widget_set_halign(g_latency_label, GTK_ALIGN_CENTER);
    gtk_box_pack_end(GTK_BOX(main_box), g_latency_label, FALSE, FALSE, 5);
    
    // Update timer (50ms = 20 FPS)
    g_timeout_add(50, update_ui, NULL);
    
    gtk_widget_show_all(window);
    gtk_main();
    
    // Cleanup
    if (g_shell) {
        venom_shell_destroy(g_shell);
    }
    
    return 0;
}
