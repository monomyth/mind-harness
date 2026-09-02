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
use crate::laterality::latch_rails;
use crate::widgets::head_plot::LABELS;
use crate::widgets::Widget;
use eframe::egui;

pub struct WImpedance {
    title: String,
    last_values: Vec<Option<f64>>,
    pending_start: bool,
    pending_stop: bool,
    /// Last Start / scan error; kept until the next successful Start.
    last_error: Option<String>,
    /// Railed (contact lost) channels — same detection as Head Plot.
    railed: [bool; 8],
}

impl WImpedance {
    pub fn new() -> Self {
        Self {
            title: "Impedance".to_string(),
            last_values: vec![],
            pending_start: false,
            pending_stop: false,
            last_error: None,
            railed: [false; 8],
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

    pub fn set_start_error(&mut self, err: impl Into<String>) {
        self.last_error = Some(err.into());
    }

    pub fn clear_start_error(&mut self) {
        self.last_error = None;
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
        let exg = source.exg_channels();
        let n = (source.sample_rate() as f64 * crate::laterality::WINDOW_SEC) as usize;
        let data = source.get_data(n.max(32));
        let chs: Vec<Vec<f64>> = exg
            .iter()
            .take(8)
            .map(|&col| {
                data.iter()
                    .map(|row| row.get(col).copied().unwrap_or(0.0))
                    .collect()
            })
            .collect();
        latch_rails(&mut self.railed, &chs);
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
                let site = LABELS.get(ch).copied().unwrap_or("—");
                ui.small(format!("measuring {} (ADS1299 lead-off)", site));
            }
        });

        if let Some(err) = &self.last_error {
            ui.colored_label(egui::Color32::from_rgb(230, 80, 70), err);
        }

        ui.add_space(4.0);

        if self.last_values.is_empty() {
            ui.small(format!("Waiting for {} channel readings…", n));
            return;
        }

        let (green_max, yellow_max) = source.impedance_quality_kohm();

        egui::Grid::new("imp_grid").striped(true).show(ui, |ui| {
            ui.strong("Site");
            ui.strong("Impedance (kΩ)");
            ui.strong("Quality");
            ui.end_row();

            for (i, val) in self.last_values.iter().enumerate().take(n) {
                let label = LABELS.get(i).copied().unwrap_or_else(|| "—");
                let is_railed = self.railed.get(i).copied().unwrap_or(false);
                ui.label(label);
                if is_railed {
                    let stop = egui::Color32::from_rgb(0xc8, 0x50, 0x50);
                    ui.colored_label(stop, "Contact lost");
                    ui.label("");
                } else {
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
