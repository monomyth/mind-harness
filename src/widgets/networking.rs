//! Networking Widget — allows configuring and enabling live data streaming
//! (LSL, UDP, OSC). This is the Rust port of the original W_Networking.

use crate::board::DataSource;
use crate::networking::Protocol;
use crate::widgets::Widget;
use eframe::egui;

pub struct WNetworking {
    title: String,
    // UI-editable state (synced from the real NetworkingManager via WidgetContext each frame)
    udp_target: String,
    osc_target: String,
    udp_enabled: bool,
    osc_enabled: bool,
}

impl WNetworking {
    pub fn new() -> Self {
        Self {
            title: "Networking".to_string(),
            // Match the defaults in NetworkingManager::new()
            udp_target: "127.0.0.1:12345".to_string(),
            osc_target: "127.0.0.1:9000".to_string(),
            udp_enabled: false,
            osc_enabled: false,
        }
    }
}

impl Default for WNetworking {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WNetworking {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, _source: &dyn DataSource) {}

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        _source: &dyn DataSource,
        ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.label("Live EEG + marker output (UDP / OSC). LSL is not built in this binary.");
        if let Some(err) = ctx.networking.last_error.clone() {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
        }
        ui.separator();

        // Sync our editable state from the real manager (source of truth) every frame.
        // This lets top-bar toggles and the widget stay in sync.
        for cfg in &ctx.networking.configs {
            match cfg.protocol {
                Protocol::UDP => {
                    self.udp_enabled = cfg.enabled;
                    if !cfg.target.is_empty() {
                        self.udp_target = cfg.target.clone();
                    }
                }
                Protocol::OSC => {
                    self.osc_enabled = cfg.enabled;
                    if !cfg.target.is_empty() {
                        self.osc_target = cfg.target.clone();
                    }
                }
                Protocol::LSL => { /* read-only for now */ }
            }
        }

        // ========== UDP Section ==========
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.udp_enabled, "UDP").changed() {
                // Live toggle feels good; apply immediately
                ctx.update_networking_config(Protocol::UDP, self.udp_enabled, &self.udp_target);
                ctx.apply_networking();
            }
            ui.strong("Target:");
            let te = ui.add(egui::TextEdit::singleline(&mut self.udp_target).desired_width(160.0));
            if te.changed() {
                // Don't spam apply on every keystroke; user can hit Apply or we auto on blur
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Apply UDP").clicked() {
                ctx.update_networking_config(Protocol::UDP, self.udp_enabled, &self.udp_target);
                ctx.apply_networking();
            }
            if self.udp_enabled && ctx.networking.udp_connected() {
                ui.colored_label(
                    egui::Color32::from_rgb(80, 200, 120),
                    "● Bound — sending CSV",
                )
            } else if self.udp_enabled {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 80, 80),
                    "⚠ enabled but not connected",
                )
            } else {
                ui.colored_label(egui::Color32::from_gray(140), "○ Disabled")
            }
        });
        ui.small("Format: timestamp, ch0,ch1,... or MARKER,ts,text lines. Test with: nc -ul 12345");

        ui.add_space(6.0);

        // ========== OSC Section ==========
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.osc_enabled, "OSC").changed() {
                ctx.update_networking_config(Protocol::OSC, self.osc_enabled, &self.osc_target);
                ctx.apply_networking();
            }
            ui.strong("Target:");
            let te = ui.add(egui::TextEdit::singleline(&mut self.osc_target).desired_width(160.0));
            if te.changed() {}
        });
        ui.horizontal(|ui| {
            if ui.button("Apply OSC").clicked() {
                ctx.update_networking_config(Protocol::OSC, self.osc_enabled, &self.osc_target);
                ctx.apply_networking();
            }
            if self.osc_enabled && ctx.networking.osc_connected() {
                ui.colored_label(
                    egui::Color32::from_rgb(80, 200, 120),
                    "● Bound — /openbci/eeg + /marker",
                )
            } else if self.osc_enabled {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 80, 80),
                    "⚠ enabled but not connected",
                )
            } else {
                ui.colored_label(egui::Color32::from_gray(140), "○ Disabled")
            }
        });
        ui.small("OSC address: /openbci/eeg (floats) and /openbci/marker (string + float ts). Works with Max, Pd, SuperCollider, etc.");

        ui.add_space(6.0);

        // ========== LSL (placeholder, non-functional) ==========
        ui.horizontal(|ui| {
            let mut lsl_off = false;
            ui.add_enabled(false, egui::Checkbox::new(&mut lsl_off, "LSL"));
            ui.strong("Target:");
            ui.add(
                egui::TextEdit::singleline(&mut "OpenBCI_EEG".to_string())
                    .desired_width(160.0)
                    .interactive(false),
            );
            ui.colored_label(egui::Color32::from_rgb(220, 140, 60), "Planned");
        });
        ui.small("LSL will use the official lsl crate once build issues on macOS are resolved.");

        ui.add_space(10.0);
        ui.separator();

        // Test marker from within the widget (uses the same ctx path as the Marker widget)
        if ui.button("Send Test Marker via Network").clicked() {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            ctx.send_marker(ts, "NetworkingTest");
        }
        ui.small("Markers typed in the Marker widget are automatically forwarded to any enabled UDP/OSC stream + recording annotations.");

        ui.add_space(4.0);
        ui.label("Data sent: EEG samples (from board) + Markers (from Marker widget or the button above).");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
