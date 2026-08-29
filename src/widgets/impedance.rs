//! WImpedance — hardware impedance test UI (Cyton 8/16ch + Ganglion).
//!
//! Matches the spirit of the original OpenBCI GUI "Impedance" / "Hardware Settings" flow.
//! - Start / Stop buttons drive the board into impedance measurement mode.
//! - Per-channel readings (kΩ) with classic color coding (green good, yellow marginal, red poor).
//! - Live Cyton/Ganglion: kΩ from ADS1299 lead-off / Ganglion resistance columns (never faked).
//! - Synthetic and Playback keep labelled simulated readings.
//! - The widget requests actions via internal flags; the app polls with downcast_mut and
//!   calls the DataSource mut methods. This keeps the Widget trait surface unchanged.

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;

pub struct WImpedance {
    title: String,
    last_values: Vec<Option<f64>>,
    pending_start: bool,
    pending_stop: bool,
}

impl WImpedance {
    pub fn new() -> Self {
        Self {
            title: "Impedance".to_string(),
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
        if source.supports_impedance() {
            self.last_values = source.get_impedance();
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
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

        let testing = source.impedance_test_active();
        ui.horizontal(|ui| {
            if !testing {
                if ui.button("▶ Start Impedance Test").clicked() {
                    self.pending_start = true;
                }
            } else if ui.button("⏹ Stop Test").clicked() {
                self.pending_stop = true;
            }
            if testing {
                ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "● Testing...");
            }
            if let Some(ch) = source.impedance_scan_channel() {
                ui.small(format!("measuring ch {} (ADS1299 lead-off)", ch + 1));
            }
        });

        ui.add_space(4.0);

        if self.last_values.is_empty() {
            ui.small(format!("Waiting for {} channel readings…", n));
            return;
        }

        let (green_max, yellow_max) = source.impedance_quality_kohm();

        egui::Grid::new("imp_grid").striped(true).show(ui, |ui| {
            ui.strong("Ch");
            ui.strong("Impedance (kΩ)");
            ui.strong("Quality");
            ui.end_row();

            for (i, val) in self.last_values.iter().enumerate().take(n) {
                ui.label(format!("{}", i + 1));
                match val {
                    Some(v) if *v > 0.0 => {
                        let v = *v;
                        let (color, qual) = if v < green_max {
                            (egui::Color32::from_rgb(60, 180, 90), "Good")
                        } else if v < yellow_max {
                            (egui::Color32::from_rgb(230, 180, 60), "OK")
                        } else {
                            (egui::Color32::from_rgb(230, 80, 70), "Poor / dry")
                        };
                        ui.colored_label(color, format!("{:.1}", v));
                        ui.colored_label(color, qual);
                    }
                    _ => {
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

        if source.impedance_is_simulated() {
            ui.small(
                "Typical dry-electrode targets: < 5 kΩ excellent, 5–15 acceptable, >15 re-prep.",
            );
        } else {
            ui.small(format!(
                "Live lead-off (not simulated): < {:.0} kΩ good, {:.0}–{:.0} acceptable.",
                green_max, green_max, yellow_max
            ));
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
