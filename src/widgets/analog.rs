//! W_AnalogRead — Cyton analog aux traces (Java `W_AnalogRead`). Hidden on Synthetic.

use crate::board::DataSource;
use crate::widgets::{lock_plot_interaction, Widget};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

pub struct WAnalogRead {
    title: String,
    history: Vec<Vec<f64>>,
    pending_mode: Option<u8>,
}

impl WAnalogRead {
    pub fn new() -> Self {
        Self {
            title: "Analog Read".to_string(),
            history: Vec::new(),
            pending_mode: None,
        }
    }
    pub fn take_pending_mode(&mut self) -> Option<u8> {
        self.pending_mode.take()
    }
}

impl Default for WAnalogRead {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WAnalogRead {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let chans = source.analog_channels();
        if chans.is_empty() || !source.supports_aux_widgets() {
            return;
        }
        let n = source.recent_samples_delivered();
        if n == 0 {
            return;
        }
        let rows = source.get_raw_data(n);
        for row in rows {
            let v: Vec<f64> = chans
                .iter()
                .map(|&c| row.get(c).copied().unwrap_or(0.0))
                .collect();
            self.history.push(v);
        }
        if self.history.len() > 750 {
            let extra = self.history.len() - 750;
            self.history.drain(0..extra);
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        if !source.supports_aux_widgets() {
            ui.label("no aux — Analog Read is Cyton-only (not faked on Synthetic)");
            return;
        }
        let chans = source.analog_channels();
        if chans.is_empty() {
            ui.label("no aux — this board has no analog channels");
            return;
        }
        ui.horizontal(|ui| {
            if source.cyton_board_mode() != Some(2) {
                if ui.button("Turn Analog Read On").clicked() {
                    self.pending_mode = Some(2);
                }
                ui.small("sends /2 (Java CytonBoardMode.ANALOG)");
            } else {
                if ui.button("Default mode").clicked() {
                    self.pending_mode = Some(0);
                }
                ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "● analog");
            }
        });
        if self.history.is_empty() {
            ui.small("Waiting for analog samples…");
            return;
        }
        let plot = lock_plot_interaction(
            Plot::new("analog_plot")
                .allow_boxed_zoom(false)
                .height(140.0),
        );
        plot.show(ui, |plot_ui| {
            let n_ch = self.history[0].len();
            for ch in 0..n_ch {
                let pts: PlotPoints = self
                    .history
                    .iter()
                    .enumerate()
                    .map(|(i, v)| [i as f64, v.get(ch).copied().unwrap_or(0.0)])
                    .collect();
                plot_ui.line(Line::new(format!("A{}", ch + 5), pts));
            }
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
