//! WImpedance — hardware impedance test UI (Cyton 8/16ch + Ganglion).
//!
//! Matches the spirit of the original OpenBCI GUI "Impedance" / "Hardware Settings" flow.
//! - Start / Stop buttons drive the board into impedance measurement mode.
//! - Per-channel readings (kΩ) with classic color coding (green good, yellow marginal, red poor).
//! - Works for live hardware (when BrainFlow supports) and beautifully in Playback (simulated values).
//! - The widget requests actions via internal flags; the app polls with downcast_mut and
//!   calls the DataSource mut methods. This keeps the Widget trait surface unchanged.

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;

pub struct WImpedance {
    title: String,
    testing: bool,
    last_values: Vec<Option<f64>>,
    pending_start: bool,
    pending_stop: bool,
}

impl WImpedance {
    pub fn new() -> Self {
        Self {
            title: "Impedance".to_string(),
            testing: false,
            last_values: vec![],
            pending_start: false,
            pending_stop: false,
        }
    }

    /// Called by app each frame (via downcast_mut on tool_widgets) to decide whether
    /// to call start/stop on the real board.
    pub fn wants_start(&self) -> bool {
        self.pending_start
    }
    pub fn wants_stop(&self) -> bool {
        self.pending_stop
    }
    pub fn clear_pending(&mut self) {
        self.pending_start = false;
        self.pending_stop = false;
    }
}

impl Default for WImpedance {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WImpedance {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        // Always pull latest (cheap) so the UI shows numbers even when not "testing"
        if source.supports_impedance() {
            self.last_values = source.get_impedance();
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let supports = source.supports_impedance();
        let n = source.exg_channels().len();

        if !supports {
            ui.colored_label(
                egui::Color32::from_rgb(200, 160, 80),
                "Impedance test not supported on this board (use Cyton or Ganglion)",
            );
            return;
        }

        if source.impedance_is_simulated() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 40),
                "Simulated readings (not live hardware kΩ)",
            );
        }

        ui.horizontal(|ui| {
            if !self.testing {
                if ui.button("▶ Start Impedance Test").clicked() {
                    self.pending_start = true;
                    self.testing = true;
                    ctx.log_event(
                        crate::event_log::LogLevel::Info,
                        "Impedance",
                        "Impedance test started",
                    );
                }
            } else {
                if ui.button("⏹ Stop Test").clicked() {
                    self.pending_stop = true;
                    self.testing = false;
                    ctx.log_event(
                        crate::event_log::LogLevel::Info,
                        "Impedance",
                        "Impedance test stopped",
                    );
                }
            }
            if self.testing {
                ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "● Testing...");
            }
        });

        ui.add_space(4.0);

        if self.last_values.is_empty() {
            ui.small(format!("Waiting for {} channel readings…", n));
            return;
        }

        // Classic per-channel readout table
        egui::Grid::new("imp_grid").striped(true).show(ui, |ui| {
            ui.strong("Ch");
            ui.strong("Impedance (kΩ)");
            ui.strong("Quality");
            ui.end_row();

            for (i, val) in self.last_values.iter().enumerate().take(n) {
                ui.label(format!("{}", i + 1));
                match val {
                    Some(v) => {
                        let v = *v;
                        let (color, qual) = if v < 5.0 {
                            (egui::Color32::from_rgb(60, 180, 90), "Good")
                        } else if v < 15.0 {
                            (egui::Color32::from_rgb(230, 180, 60), "OK")
                        } else {
                            (egui::Color32::from_rgb(230, 80, 70), "Poor / dry")
                        };
                        ui.colored_label(color, format!("{:.1}", v));
                        ui.colored_label(color, qual);
                    }
                    None => {
                        ui.label("—");
                        ui.label(if source.impedance_is_simulated() {
                            "n/a"
                        } else {
                            "no hardware reading"
                        });
                    }
                }
                ui.end_row();
            }
        });

        ui.small("Typical dry-electrode targets: < 5 kΩ excellent, 5–15 acceptable, >15 re-prep.");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
