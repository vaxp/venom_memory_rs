/**
 * VenomMemory GTK3 GUI Monitor
 * 
 * A graphical system monitor reading from Rust daemon via C bindings.
 * Tests the smoothness of GUI updates with VenomMemory IPC.
 * 
 * Build: gcc -O2 `pkg-config --cflags gtk+-3.0` c_gui_monitor.c -o c_gui_monitor \
 *        `pkg-config --libs gtk+-3.0` -L./target/release -lvenom_memory -Wl,-rpath=./target/release
 */

#include <gtk/gtk.h>
#include <stdint.h>
#include "venom_memory_rs.h"

// Must match Rust SystemStats struct
typedef struct {
    float cpu_usage_percent;
    float cpu_cores[16];
    uint32_t core_count;
    uint32_t memory_used_mb;
    uint32_t memory_total_mb;
    uint64_t uptime_seconds;
    uint64_t timestamp_ns;
} SystemStats;

// Global state
VenomShellHandle* g_shell = NULL;
GtkProgressBar* g_cpu_bar = NULL;
GtkProgressBar* g_core_bars[16];
GtkProgressBar* g_ram_bar = NULL;
GtkLabel* g_cpu_label = NULL;
GtkLabel* g_ram_label = NULL;
GtkLabel* g_uptime_label = NULL;
GtkLabel* g_status_label = NULL;
uint32_t g_core_count = 0;
int g_frame = 0;

static gboolean update_stats(gpointer user_data) {
    if (!g_shell) return G_SOURCE_CONTINUE;
    
    uint8_t buf[256];
    size_t len = venom_shell_read_data(g_shell, buf, sizeof(buf));
    
    if (len >= sizeof(SystemStats)) {
        SystemStats* stats = (SystemStats*)buf;
        g_frame++;
        
        // Update CPU total
        gtk_progress_bar_set_fraction(g_cpu_bar, stats->cpu_usage_percent / 100.0);
        char cpu_text[64];
        snprintf(cpu_text, sizeof(cpu_text), "CPU: %.1f%%", stats->cpu_usage_percent);
        gtk_label_set_text(g_cpu_label, cpu_text);
        
        // Update per-core bars
        g_core_count = stats->core_count > 16 ? 16 : stats->core_count;
        for (uint32_t i = 0; i < g_core_count; i++) {
            gtk_progress_bar_set_fraction(g_core_bars[i], stats->cpu_cores[i] / 100.0);
            char core_text[32];
            snprintf(core_text, sizeof(core_text), "Core %u: %.0f%%", i, stats->cpu_cores[i]);
            gtk_progress_bar_set_text(g_core_bars[i], core_text);
            gtk_widget_show(GTK_WIDGET(g_core_bars[i]));
        }
        for (uint32_t i = g_core_count; i < 16; i++) {
            gtk_widget_hide(GTK_WIDGET(g_core_bars[i]));
        }
        
        // Update RAM
        float ram_pct = 0;
        if (stats->memory_total_mb > 0) {
            ram_pct = (float)stats->memory_used_mb / (float)stats->memory_total_mb;
        }
        gtk_progress_bar_set_fraction(g_ram_bar, ram_pct);
        char ram_text[128];
        snprintf(ram_text, sizeof(ram_text), "RAM: %u / %u MB (%.0f%%)", 
            stats->memory_used_mb, stats->memory_total_mb, ram_pct * 100);
        gtk_label_set_text(g_ram_label, ram_text);
        
        // Update uptime
        uint64_t s = stats->uptime_seconds;
        uint64_t d = s / 86400;
        uint64_t h = (s % 86400) / 3600;
        uint64_t m = (s % 3600) / 60;
        char uptime_text[64];
        snprintf(uptime_text, sizeof(uptime_text), "⏱️ Uptime: %lud %luh %lum", 
            (unsigned long)d, (unsigned long)h, (unsigned long)m);
        gtk_label_set_text(g_uptime_label, uptime_text);
        
        // Update status
        char status_text[128];
        snprintf(status_text, sizeof(status_text), 
            "Frame: %d | Cores: %u | VenomMemory C→Rust IPC", 
            g_frame, stats->core_count);
        gtk_label_set_text(g_status_label, status_text);
    }
    
    return G_SOURCE_CONTINUE;
}

int main(int argc, char** argv) {
    gtk_init(&argc, &argv);
    
    // Connect to VenomMemory
    g_shell = venom_shell_connect("system_monitor");
    if (!g_shell) {
        g_print("❌ Failed to connect! Run: cargo run --release --example system_daemon\n");
    }
    
    // Create window
    GtkWidget* window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "VenomMemory C GUI Monitor");
    gtk_window_set_default_size(GTK_WINDOW(window), 450, 550);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    
    // Main box
    GtkWidget* main_box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    gtk_container_set_border_width(GTK_CONTAINER(main_box), 15);
    gtk_container_add(GTK_CONTAINER(window), main_box);
    
    // Title
    GtkWidget* title = gtk_label_new(NULL);
    gtk_label_set_markup(GTK_LABEL(title), 
        "<span size='x-large' weight='bold'>🖥️ VenomMemory C GUI Monitor</span>");
    gtk_box_pack_start(GTK_BOX(main_box), title, FALSE, FALSE, 10);
    
    // CPU section
    g_cpu_label = GTK_LABEL(gtk_label_new("CPU: 0%"));
    gtk_widget_set_halign(GTK_WIDGET(g_cpu_label), GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_cpu_label), FALSE, FALSE, 0);
    
    g_cpu_bar = GTK_PROGRESS_BAR(gtk_progress_bar_new());
    gtk_progress_bar_set_show_text(g_cpu_bar, TRUE);
    gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_cpu_bar), FALSE, FALSE, 0);
    
    // Separator
    gtk_box_pack_start(GTK_BOX(main_box), gtk_separator_new(GTK_ORIENTATION_HORIZONTAL), FALSE, FALSE, 5);
    
    // Core label
    GtkWidget* cores_label = gtk_label_new("Per-Core Usage:");
    gtk_widget_set_halign(cores_label, GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(main_box), cores_label, FALSE, FALSE, 0);
    
    // Per-core bars
    for (int i = 0; i < 16; i++) {
        g_core_bars[i] = GTK_PROGRESS_BAR(gtk_progress_bar_new());
        gtk_progress_bar_set_show_text(g_core_bars[i], TRUE);
        gtk_widget_set_no_show_all(GTK_WIDGET(g_core_bars[i]), TRUE);
        gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_core_bars[i]), FALSE, FALSE, 2);
    }
    
    // Separator
    gtk_box_pack_start(GTK_BOX(main_box), gtk_separator_new(GTK_ORIENTATION_HORIZONTAL), FALSE, FALSE, 5);
    
    // RAM section
    g_ram_label = GTK_LABEL(gtk_label_new("RAM: 0 / 0 MB"));
    gtk_widget_set_halign(GTK_WIDGET(g_ram_label), GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_ram_label), FALSE, FALSE, 0);
    
    g_ram_bar = GTK_PROGRESS_BAR(gtk_progress_bar_new());
    gtk_progress_bar_set_show_text(g_ram_bar, TRUE);
    gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_ram_bar), FALSE, FALSE, 0);
    
    // Separator
    gtk_box_pack_start(GTK_BOX(main_box), gtk_separator_new(GTK_ORIENTATION_HORIZONTAL), FALSE, FALSE, 5);
    
    // Uptime
    g_uptime_label = GTK_LABEL(gtk_label_new("⏱️ Uptime: 0h 0m"));
    gtk_widget_set_halign(GTK_WIDGET(g_uptime_label), GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_uptime_label), FALSE, FALSE, 0);
    
    // Status
    g_status_label = GTK_LABEL(gtk_label_new("Connecting..."));
    gtk_widget_set_halign(GTK_WIDGET(g_status_label), GTK_ALIGN_CENTER);
    gtk_box_pack_start(GTK_BOX(main_box), GTK_WIDGET(g_status_label), FALSE, FALSE, 10);
    
    // Timer for updates (100ms = 10 FPS)
    g_timeout_add(100, update_stats, NULL);
    
    gtk_widget_show_all(window);
    gtk_main();
    
    if (g_shell) {
        venom_shell_destroy(g_shell);
    }
    
    return 0;
}
