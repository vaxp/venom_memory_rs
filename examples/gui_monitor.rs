//! GUI System Monitor - VenomMemory + egui Demo

use eframe::egui;
use venom_memory::ShellChannel;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SystemStats {
    pub cpu_usage_percent: f32,
    pub cpu_cores: [f32; 16],
    pub core_count: u32,
    pub memory_used_mb: u32,
    pub memory_total_mb: u32,
    pub uptime_seconds: u64,
    pub timestamp_ns: u64,
}

struct Monitor {
    shell: Option<ShellChannel>,
    stats: SystemStats,
}

impl Monitor {
    fn new() -> Self {
        Self {
            shell: ShellChannel::connect("system_monitor").ok(),
            stats: SystemStats::default(),
        }
    }
}

impl eframe::App for Monitor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Read from VenomMemory
        if let Some(ref shell) = self.shell {
            let mut buf = vec![0u8; std::mem::size_of::<SystemStats>() + 64];
            let len = shell.read_data(&mut buf);
            if len >= std::mem::size_of::<SystemStats>() {
                self.stats = unsafe { std::ptr::read(buf.as_ptr() as *const SystemStats) };
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🖥️ VenomMemory System Monitor");
            ui.separator();

            if self.shell.is_some() {
                // CPU
                ui.label(format!("CPU Total: {:.1}%", self.stats.cpu_usage_percent));
                ui.add(egui::ProgressBar::new(self.stats.cpu_usage_percent / 100.0).show_percentage());
                
                ui.separator();
                
                // Per-core
                let cores = (self.stats.core_count as usize).min(8);
                for i in 0..cores {
                    ui.horizontal(|ui| {
                        ui.label(format!("Core {}: ", i));
                        ui.add(egui::ProgressBar::new(self.stats.cpu_cores[i] / 100.0)
                            .desired_width(200.0));
                        ui.label(format!("{:.1}%", self.stats.cpu_cores[i]));
                    });
                }
                
                ui.separator();
                
                // RAM
                let mem_pct = self.stats.memory_used_mb as f32 / self.stats.memory_total_mb as f32;
                ui.label(format!("RAM: {} / {} MB", self.stats.memory_used_mb, self.stats.memory_total_mb));
                ui.add(egui::ProgressBar::new(mem_pct).show_percentage());
                
                ui.separator();
                
                // Uptime
                let s = self.stats.uptime_seconds;
                let h = s / 3600;
                let m = (s % 3600) / 60;
                ui.label(format!("⏱️ Uptime: {}h {}m", h, m));
            } else {
                ui.colored_label(egui::Color32::RED, "❌ Not connected");
                ui.label("Run: cargo run --release --example system_daemon");
            }
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "VenomMemory Monitor",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 450.0]),
            ..Default::default()
        },
        Box::new(|_cc| Box::new(Monitor::new())),
    )
}
