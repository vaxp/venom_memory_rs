#include "audio.h"
#include "venom_ipc.h"
#include <stdio.h>
#include <signal.h>
#include <unistd.h>
#include <time.h>

// ═══════════════════════════════════════════════════════════════════════════
// 🌐 Global State
// ═══════════════════════════════════════════════════════════════════════════

static volatile int running = 1;

// ═══════════════════════════════════════════════════════════════════════════
// 🛑 Signal Handler
// ═══════════════════════════════════════════════════════════════════════════

static void signal_handler(int sig) {
    (void)sig;
    printf("\n🛑 Shutting down...\n");
    running = 0;
}

// ═══════════════════════════════════════════════════════════════════════════
// 🚀 Main
// ═══════════════════════════════════════════════════════════════════════════

int main(void) {
    printf("🔊 ═══════════════════════════════════════════════════════════════\n");
    printf("🔊 Venom Audio Daemon v2.0 (VenomMemory IPC)\n");
    printf("🔊 ═══════════════════════════════════════════════════════════════\n");
    
    // Signal handling
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);
    
    // Initialize audio
    if (!audio_init()) {
        printf("❌ Failed to initialize audio\n");
        return 1;
    }
    
    // Initialize VenomMemory IPC
    if (!venom_ipc_init()) {
        printf("❌ Failed to initialize VenomMemory IPC\n");
        audio_cleanup();
        return 1;
    }
    
    // Set callbacks
    audio_state.on_volume_changed = venom_on_volume_changed;
    audio_state.on_mute_changed = venom_on_mute_changed;
    audio_state.on_devices_changed = venom_on_devices_changed;
    audio_state.on_apps_changed = venom_on_apps_changed;
    
    printf("🚀 Daemon running... (Press Ctrl+C to stop)\n");
    printf("📡 Channel: /dev/shm/venom_venom_audio\n");
    
    // Main loop - publish state periodically and process commands
    struct timespec last_publish = {0};
    clock_gettime(CLOCK_MONOTONIC, &last_publish);
    
    while (running) {
        struct timespec now;
        clock_gettime(CLOCK_MONOTONIC, &now);
        
        // Publish state every 100ms
        long elapsed_ms = (now.tv_sec - last_publish.tv_sec) * 1000 +
                          (now.tv_nsec - last_publish.tv_nsec) / 1000000;
        
        if (elapsed_ms >= 100) {
            venom_publish_state();
            last_publish = now;
        }
        
        // Handle deferred updates from callbacks (safe thread)
        venom_ipc_sync();
        
        // Process incoming commands from clients
        venom_poll_commands();
        
        // Small sleep to avoid busy-waiting
        usleep(5000);  // 5ms (increased frequency for smoother UI)
    }
    
    // Cleanup
    venom_ipc_cleanup();
    audio_cleanup();
    
    printf("👋 Goodbye!\n");
    return 0;
}
