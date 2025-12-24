#include "audio.h"
#include <stdio.h>
#include <string.h>

// ═══════════════════════════════════════════════════════════════════════════
// 🔊 Global State
// ═══════════════════════════════════════════════════════════════════════════

AudioState audio_state = {0};

// ═══════════════════════════════════════════════════════════════════════════
// 🔧 Helper Functions
// ═══════════════════════════════════════════════════════════════════════════

static gint pa_volume_to_percent(pa_volume_t vol) {
    return (gint)((vol * 100) / PA_VOLUME_NORM);
}

static pa_volume_t percent_to_pa_volume(gint percent) {
    return (pa_volume_t)((percent * PA_VOLUME_NORM) / 100);
}

void audio_device_free(AudioDevice *dev) {
    if (dev) {
        g_free(dev->name);
        g_free(dev->description);
        g_free(dev);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 📡 PulseAudio Callbacks
// ═══════════════════════════════════════════════════════════════════════════

static void server_info_cb(pa_context *c, const pa_server_info *info, void *userdata) {
    (void)c; (void)userdata;
    if (!info) return;
    
    g_free(audio_state.default_sink);
    g_free(audio_state.default_source);
    audio_state.default_sink = g_strdup(info->default_sink_name);
    audio_state.default_source = g_strdup(info->default_source_name);
    
    printf("🔊 Default sink: %s\n", audio_state.default_sink);
    printf("🎤 Default source: %s\n", audio_state.default_source);
    
    pa_threaded_mainloop_signal(audio_state.mainloop, 0);
}

static void sink_info_cb(pa_context *c, const pa_sink_info *info, int eol, void *userdata) {
    (void)c;
    if (eol > 0) {
        pa_threaded_mainloop_signal(audio_state.mainloop, 0);
        return;
    }
    if (!info) return;
    
    GList **list = (GList**)userdata;
    if (list) {
        AudioDevice *dev = g_new0(AudioDevice, 1);
        dev->name = g_strdup(info->name);
        dev->description = g_strdup(info->description);
        dev->volume = pa_volume_to_percent(pa_cvolume_avg(&info->volume));
        dev->muted = info->mute;
        dev->is_default = (audio_state.default_sink && 
                          g_strcmp0(info->name, audio_state.default_sink) == 0);
        *list = g_list_append(*list, dev);
    } else {
        // Update default sink info
        if (audio_state.default_sink && g_strcmp0(info->name, audio_state.default_sink) == 0) {
            audio_state.volume = pa_volume_to_percent(pa_cvolume_avg(&info->volume));
            audio_state.muted = info->mute;
        }
    }
}

static void source_info_cb(pa_context *c, const pa_source_info *info, int eol, void *userdata) {
    (void)c;
    if (eol > 0) {
        pa_threaded_mainloop_signal(audio_state.mainloop, 0);
        return;
    }
    if (!info) return;
    
    // Skip monitors
    if (strstr(info->name, ".monitor")) return;
    
    GList **list = (GList**)userdata;
    if (list) {
        AudioDevice *dev = g_new0(AudioDevice, 1);
        dev->name = g_strdup(info->name);
        dev->description = g_strdup(info->description);
        dev->volume = pa_volume_to_percent(pa_cvolume_avg(&info->volume));
        dev->muted = info->mute;
        dev->is_default = (audio_state.default_source && 
                          g_strcmp0(info->name, audio_state.default_source) == 0);
        *list = g_list_append(*list, dev);
    } else {
        // Update default source info
        if (audio_state.default_source && g_strcmp0(info->name, audio_state.default_source) == 0) {
            audio_state.mic_volume = pa_volume_to_percent(pa_cvolume_avg(&info->volume));
            audio_state.mic_muted = info->mute;
        }
    }
}

static void success_cb(pa_context *c, int success, void *userdata) {
    (void)c; (void)success; (void)userdata;
    pa_threaded_mainloop_signal(audio_state.mainloop, 0);
}

static void subscribe_cb(pa_context *c, pa_subscription_event_type_t type, uint32_t idx, void *userdata) {
    (void)c; (void)idx; (void)userdata;
    
    unsigned facility = type & PA_SUBSCRIPTION_EVENT_FACILITY_MASK;
    
    if (facility == PA_SUBSCRIPTION_EVENT_SINK) {
        // Refresh sink info
        pa_context_get_sink_info_by_name(audio_state.context, audio_state.default_sink, sink_info_cb, NULL);
        if (audio_state.on_volume_changed) {
            audio_state.on_volume_changed(audio_state.volume);
        }
    } else if (facility == PA_SUBSCRIPTION_EVENT_SOURCE) {
        // Refresh source info
        pa_context_get_source_info_by_name(audio_state.context, audio_state.default_source, source_info_cb, NULL);
    } else if (facility == PA_SUBSCRIPTION_EVENT_SERVER) {
        // Server changed (default devices may have changed)
        pa_context_get_server_info(audio_state.context, server_info_cb, NULL);
        if (audio_state.on_devices_changed) {
            audio_state.on_devices_changed();
        }
    } else if (facility == PA_SUBSCRIPTION_EVENT_SINK_INPUT) {
        // Application streams changed
        if (audio_state.on_apps_changed) {
            audio_state.on_apps_changed();
        }
    }
}

static void context_state_cb(pa_context *c, void *userdata) {
    (void)userdata;
    
    switch (pa_context_get_state(c)) {
        case PA_CONTEXT_READY:
            printf("🔊 PulseAudio connected\n");
            audio_state.ready = TRUE;
            
            // Subscribe to events
            pa_context_set_subscribe_callback(c, subscribe_cb, NULL);
            pa_context_subscribe(c, PA_SUBSCRIPTION_MASK_SINK | 
                                   PA_SUBSCRIPTION_MASK_SOURCE | 
                                   PA_SUBSCRIPTION_MASK_SERVER |
                                   PA_SUBSCRIPTION_MASK_SINK_INPUT, NULL, NULL);
            
            // Get initial info
            pa_context_get_server_info(c, server_info_cb, NULL);
            break;
            
        case PA_CONTEXT_FAILED:
        case PA_CONTEXT_TERMINATED:
            printf("❌ PulseAudio connection failed\n");
            audio_state.ready = FALSE;
            pa_threaded_mainloop_signal(audio_state.mainloop, 0);
            break;
            
        default:
            break;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 🚀 Initialization
// ═══════════════════════════════════════════════════════════════════════════

gboolean audio_init(void) {
    audio_state.mainloop = pa_threaded_mainloop_new();
    if (!audio_state.mainloop) {
        printf("❌ Failed to create mainloop\n");
        return FALSE;
    }
    
    pa_mainloop_api *api = pa_threaded_mainloop_get_api(audio_state.mainloop);
    audio_state.context = pa_context_new(api, "venom_audio");
    if (!audio_state.context) {
        printf("❌ Failed to create context\n");
        pa_threaded_mainloop_free(audio_state.mainloop);
        return FALSE;
    }
    
    pa_context_set_state_callback(audio_state.context, context_state_cb, NULL);
    
    if (pa_context_connect(audio_state.context, NULL, PA_CONTEXT_NOFLAGS, NULL) < 0) {
        printf("❌ Failed to connect to PulseAudio\n");
        pa_context_unref(audio_state.context);
        pa_threaded_mainloop_free(audio_state.mainloop);
        return FALSE;
    }
    
    pa_threaded_mainloop_start(audio_state.mainloop);
    
    // Wait for connection
    pa_threaded_mainloop_lock(audio_state.mainloop);
    while (!audio_state.ready && pa_context_get_state(audio_state.context) != PA_CONTEXT_FAILED) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    if (!audio_state.ready) {
        audio_cleanup();
        return FALSE;
    }
    
    // Get initial volume
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_context_get_sink_info_by_name(audio_state.context, audio_state.default_sink, sink_info_cb, NULL);
    pa_threaded_mainloop_wait(audio_state.mainloop);
    pa_context_get_source_info_by_name(audio_state.context, audio_state.default_source, source_info_cb, NULL);
    pa_threaded_mainloop_wait(audio_state.mainloop);
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    printf("🔊 Volume: %d%% %s\n", audio_state.volume, audio_state.muted ? "(muted)" : "");
    printf("🎤 Mic: %d%% %s\n", audio_state.mic_volume, audio_state.mic_muted ? "(muted)" : "");
    
    return TRUE;
}

void audio_cleanup(void) {
    if (audio_state.mainloop) {
        pa_threaded_mainloop_stop(audio_state.mainloop);
    }
    if (audio_state.context) {
        pa_context_disconnect(audio_state.context);
        pa_context_unref(audio_state.context);
    }
    if (audio_state.mainloop) {
        pa_threaded_mainloop_free(audio_state.mainloop);
    }
    g_free(audio_state.default_sink);
    g_free(audio_state.default_source);
    memset(&audio_state, 0, sizeof(audio_state));
}

// ═══════════════════════════════════════════════════════════════════════════
// 🔊 Volume Control
// ═══════════════════════════════════════════════════════════════════════════

gint audio_get_volume(void) {
    return audio_state.volume;
}

gboolean audio_set_volume(gint volume) {
    if (!audio_state.ready || !audio_state.default_sink) return FALSE;
    if (volume < 0) volume = 0;
    if (volume > 150) volume = 150;  // Allow boost up to 150%
    
    pa_cvolume cv;
    pa_cvolume_set(&cv, 2, percent_to_pa_volume(volume));
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_sink_volume_by_name(
        audio_state.context, audio_state.default_sink, &cv, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    audio_state.volume = volume;
    printf("🔊 Volume set to %d%%\n", volume);
    return TRUE;
}

gboolean audio_get_muted(void) {
    return audio_state.muted;
}

gboolean audio_set_muted(gboolean muted) {
    if (!audio_state.ready || !audio_state.default_sink) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_sink_mute_by_name(
        audio_state.context, audio_state.default_sink, muted, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    audio_state.muted = muted;
    printf("🔊 %s\n", muted ? "Muted" : "Unmuted");
    return TRUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🎤 Microphone Control
// ═══════════════════════════════════════════════════════════════════════════

gint audio_get_mic_volume(void) {
    return audio_state.mic_volume;
}

gboolean audio_set_mic_volume(gint volume) {
    if (!audio_state.ready || !audio_state.default_source) return FALSE;
    if (volume < 0) volume = 0;
    if (volume > 100) volume = 100;
    
    pa_cvolume cv;
    pa_cvolume_set(&cv, 2, percent_to_pa_volume(volume));
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_source_volume_by_name(
        audio_state.context, audio_state.default_source, &cv, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    audio_state.mic_volume = volume;
    printf("🎤 Mic volume set to %d%%\n", volume);
    return TRUE;
}

gboolean audio_get_mic_muted(void) {
    return audio_state.mic_muted;
}

gboolean audio_set_mic_muted(gboolean muted) {
    if (!audio_state.ready || !audio_state.default_source) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_source_mute_by_name(
        audio_state.context, audio_state.default_source, muted, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    audio_state.mic_muted = muted;
    printf("🎤 Mic %s\n", muted ? "muted" : "unmuted");
    return TRUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🔈 Sinks (Output Devices)
// ═══════════════════════════════════════════════════════════════════════════

GList* audio_get_sinks(void) {
    if (!audio_state.ready) return NULL;
    
    GList *sinks = NULL;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_get_sink_info_list(audio_state.context, sink_info_cb, &sinks);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return sinks;
}

gboolean audio_set_default_sink(const gchar *name) {
    if (!audio_state.ready || !name) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_default_sink(audio_state.context, name, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    g_free(audio_state.default_sink);
    audio_state.default_sink = g_strdup(name);
    printf("🔊 Default sink: %s\n", name);
    return TRUE;
}

gboolean audio_set_sink_volume(const gchar *name, gint volume) {
    if (!audio_state.ready || !name) return FALSE;
    if (volume < 0) volume = 0;
    if (volume > 150) volume = 150;
    
    pa_cvolume cv;
    pa_cvolume_set(&cv, 2, percent_to_pa_volume(volume));
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_sink_volume_by_name(
        audio_state.context, name, &cv, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return TRUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🎤 Sources (Input Devices)
// ═══════════════════════════════════════════════════════════════════════════

GList* audio_get_sources(void) {
    if (!audio_state.ready) return NULL;
    
    GList *sources = NULL;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_get_source_info_list(audio_state.context, source_info_cb, &sources);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return sources;
}

gboolean audio_set_default_source(const gchar *name) {
    if (!audio_state.ready || !name) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_default_source(audio_state.context, name, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    g_free(audio_state.default_source);
    audio_state.default_source = g_strdup(name);
    printf("🎤 Default source: %s\n", name);
    return TRUE;
}

gboolean audio_set_source_volume(const gchar *name, gint volume) {
    if (!audio_state.ready || !name) return FALSE;
    if (volume < 0) volume = 0;
    if (volume > 100) volume = 100;
    
    pa_cvolume cv;
    pa_cvolume_set(&cv, 2, percent_to_pa_volume(volume));
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_source_volume_by_name(
        audio_state.context, name, &cv, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return TRUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🎵 Application Streams (Sink Inputs)
// ═══════════════════════════════════════════════════════════════════════════

void audio_app_stream_free(AppStream *app) {
    if (app) {
        g_free(app->name);
        g_free(app->icon);
        g_free(app->sink_name);
        g_free(app);
    }
}

static void sink_input_info_cb(pa_context *c, const pa_sink_input_info *info, int eol, void *userdata) {
    (void)c;
    if (eol > 0) {
        pa_threaded_mainloop_signal(audio_state.mainloop, 0);
        return;
    }
    if (!info) return;
    
    GList **list = (GList**)userdata;
    if (list) {
        AppStream *app = g_new0(AppStream, 1);
        app->index = info->index;
        
        // Get application name from proplist
        const char *app_name = pa_proplist_gets(info->proplist, PA_PROP_APPLICATION_NAME);
        app->name = g_strdup(app_name ? app_name : "Unknown");
        
        // Get application icon
        const char *app_icon = pa_proplist_gets(info->proplist, PA_PROP_APPLICATION_ICON_NAME);
        app->icon = g_strdup(app_icon ? app_icon : "audio-volume-medium");
        
        app->volume = pa_volume_to_percent(pa_cvolume_avg(&info->volume));
        app->muted = info->mute;
        
        *list = g_list_append(*list, app);
    }
}

GList* audio_get_app_streams(void) {
    if (!audio_state.ready) return NULL;
    
    GList *apps = NULL;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_get_sink_input_info_list(audio_state.context, sink_input_info_cb, &apps);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return apps;
}

gboolean audio_set_app_volume(guint32 index, gint volume) {
    if (!audio_state.ready) return FALSE;
    if (volume < 0) volume = 0;
    gint max = audio_state.overamplification ? 150 : 100;
    if (volume > max) volume = max;
    
    pa_cvolume cv;
    pa_cvolume_set(&cv, 2, percent_to_pa_volume(volume));
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_sink_input_volume(
        audio_state.context, index, &cv, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    printf("🎵 App %u volume set to %d%%\n", index, volume);
    return TRUE;
}

gboolean audio_set_app_muted(guint32 index, gboolean muted) {
    if (!audio_state.ready) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_sink_input_mute(
        audio_state.context, index, muted, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    printf("🎵 App %u %s\n", index, muted ? "muted" : "unmuted");
    return TRUE;
}

gboolean audio_move_app_to_sink(guint32 index, const gchar *sink_name) {
    if (!audio_state.ready || !sink_name) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_move_sink_input_by_name(
        audio_state.context, index, sink_name, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    printf("🎵 App %u moved to %s\n", index, sink_name);
    return TRUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 📊 Audio Profiles / Cards
// ═══════════════════════════════════════════════════════════════════════════

void audio_profile_free(AudioProfile *profile) {
    if (profile) {
        g_free(profile->name);
        g_free(profile->description);
        g_free(profile);
    }
}

typedef struct {
    GList **cards;
    GList **profiles;
    const gchar *target_card;
} CardCallbackData;

static void card_info_cb(pa_context *c, const pa_card_info *info, int eol, void *userdata) {
    (void)c;
    if (eol > 0) {
        pa_threaded_mainloop_signal(audio_state.mainloop, 0);
        return;
    }
    if (!info) return;
    
    CardCallbackData *data = (CardCallbackData*)userdata;
    
    // If getting cards list
    if (data->cards) {
        AudioDevice *card = g_new0(AudioDevice, 1);
        card->name = g_strdup(info->name);
        card->description = g_strdup(info->proplist ? 
            pa_proplist_gets(info->proplist, PA_PROP_DEVICE_DESCRIPTION) : info->name);
        card->is_default = FALSE;
        *data->cards = g_list_append(*data->cards, card);
    }
    
    // If getting profiles for specific card
    if (data->profiles && data->target_card && g_strcmp0(info->name, data->target_card) == 0) {
        for (uint32_t i = 0; i < info->n_profiles; i++) {
            pa_card_profile_info2 *p = info->profiles2[i];
            AudioProfile *profile = g_new0(AudioProfile, 1);
            profile->name = g_strdup(p->name);
            profile->description = g_strdup(p->description);
            profile->available = (p->available != PA_PORT_AVAILABLE_NO);
            *data->profiles = g_list_append(*data->profiles, profile);
        }
    }
}

GList* audio_get_cards(void) {
    if (!audio_state.ready) return NULL;
    
    GList *cards = NULL;
    CardCallbackData data = { &cards, NULL, NULL };
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_get_card_info_list(audio_state.context, card_info_cb, &data);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return cards;
}

GList* audio_get_profiles(const gchar *card_name) {
    if (!audio_state.ready || !card_name) return NULL;
    
    GList *profiles = NULL;
    CardCallbackData data = { NULL, &profiles, card_name };
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_get_card_info_by_name(audio_state.context, card_name, card_info_cb, &data);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    return profiles;
}

gboolean audio_set_profile(const gchar *card_name, const gchar *profile) {
    if (!audio_state.ready || !card_name || !profile) return FALSE;
    
    pa_threaded_mainloop_lock(audio_state.mainloop);
    pa_operation *op = pa_context_set_card_profile_by_name(
        audio_state.context, card_name, profile, success_cb, NULL);
    if (op) {
        pa_threaded_mainloop_wait(audio_state.mainloop);
        pa_operation_unref(op);
    }
    pa_threaded_mainloop_unlock(audio_state.mainloop);
    
    printf("📊 Card %s profile set to %s\n", card_name, profile);
    return TRUE;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🎚️ Over-amplification
// ═══════════════════════════════════════════════════════════════════════════

gboolean audio_get_overamplification(void) {
    return audio_state.overamplification;
}

gboolean audio_set_overamplification(gboolean enabled) {
    audio_state.overamplification = enabled;
    audio_state.max_volume = enabled ? 150 : 100;
    
    // If current volume exceeds new max, reduce it
    if (!enabled && audio_state.volume > 100) {
        audio_set_volume(100);
    }
    
    printf("🎚️ Over-amplification %s (max: %d%%)\n", 
           enabled ? "enabled" : "disabled", audio_state.max_volume);
    return TRUE;
}

