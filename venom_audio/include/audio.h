#ifndef VENOM_AUDIO_H
#define VENOM_AUDIO_H

#include <glib.h>
#include <pulse/pulseaudio.h>

// ═══════════════════════════════════════════════════════════════════════════
// 🔊 Types
// ═══════════════════════════════════════════════════════════════════════════

#define UPDATE_DEVICES (1 << 0)
#define UPDATE_APPS    (1 << 1)
#define UPDATE_PUBLISH (1 << 2)

typedef struct {
    gchar *name;
    gchar *description;
    gint volume;      // 0-100 (or 150 with overamplification)
    gboolean muted;
    gboolean is_default;
} AudioDevice;

typedef struct {
    guint32 index;
    gchar *name;           // Application name
    gchar *icon;           // Application icon name
    gint volume;           // 0-100
    gboolean muted;
    gchar *sink_name;      // Output device
} AppStream;

typedef struct {
    gchar *name;
    gchar *description;
    gboolean available;
} AudioProfile;

typedef struct {
    pa_threaded_mainloop *mainloop;
    pa_context *context;
    gboolean ready;
    
    // Default sink/source
    gchar *default_sink;
    gchar *default_source;
    gint volume;
    gint mic_volume;
    gboolean muted;
    gboolean mic_muted;
    
    // Settings
    gboolean overamplification;  // Allow volume > 100%
    gint max_volume;             // 100 or 150
    
    // Callbacks
    void (*on_volume_changed)(gint volume);
    void (*on_mute_changed)(gboolean muted);
    void (*on_devices_changed)(void);
    void (*on_apps_changed)(void);

    // Internal sync
    uint32_t pending_updates;
} AudioState;

extern AudioState audio_state;

// ═══════════════════════════════════════════════════════════════════════════
// 🔧 Functions
// ═══════════════════════════════════════════════════════════════════════════

// Initialization
gboolean audio_init(void);
void audio_cleanup(void);

// Volume control
gint audio_get_volume(void);
gboolean audio_set_volume(gint volume);
gboolean audio_get_muted(void);
gboolean audio_set_muted(gboolean muted);

// Microphone
gint audio_get_mic_volume(void);
gboolean audio_set_mic_volume(gint volume);
gboolean audio_get_mic_muted(void);
gboolean audio_set_mic_muted(gboolean muted);

// Sinks (output devices)
GList* audio_get_sinks(void);
gboolean audio_set_default_sink(const gchar *name);
gboolean audio_set_sink_volume(const gchar *name, gint volume);

// Sources (input devices)
GList* audio_get_sources(void);
gboolean audio_set_default_source(const gchar *name);
gboolean audio_set_source_volume(const gchar *name, gint volume);

// 🎵 Application Streams (Sink Inputs)
GList* audio_get_app_streams(void);
gboolean audio_set_app_volume(guint32 index, gint volume);
gboolean audio_set_app_muted(guint32 index, gboolean muted);
gboolean audio_move_app_to_sink(guint32 index, const gchar *sink_name);

// 📊 Profiles
GList* audio_get_profiles(const gchar *card_name);
gboolean audio_set_profile(const gchar *card_name, const gchar *profile);
GList* audio_get_cards(void);

// 🎚️ Over-amplification
gboolean audio_get_overamplification(void);
gboolean audio_set_overamplification(gboolean enabled);

// Cleanup helpers
void audio_device_free(AudioDevice *dev);
void audio_app_stream_free(AppStream *app);
void audio_profile_free(AudioProfile *profile);

#endif // VENOM_AUDIO_H

