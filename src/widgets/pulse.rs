//! W_PulseSensor — first Cyton analog channel. No fake BPM on Synthetic.

use crate::board::DataSource;
use crate::widgets::{lock_plot_interaction, Widget};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

pub struct WPulseSensor {
    title: String,
    wave: Vec<f64>,
    last: f64,
    rising: bool,
    last_beat: std::time::Instant,
    bpm: f64,
    pending_mode: Option<u8>,
}

impl WPulseSensor {
    pub fn new() -> Self {
        Self {
            title: "Pulse Sensor".to_string(),
            wave: Vec::new(),
            last: 0.0,
            rising: false,
            last_beat: std::time::Instant::now(),
            bpm: 0.0,
            pending_mode: None,
        }
    }
    pub fn take_pending_mode(&mut self) -> Option<u8> {
        self.pending_mode.take()
    }
}

impl Default for WPulseSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WPulseSensor {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        if !source.supports_aux_widgets() {
            return;
        }
        let chans = source.analog_channels();
        let Some(&col) = chans.first() else {
            return;
        };
        let n = source.recent_samples_delivered();
        if n == 0 {
            return;
        }
        for row in source.get_raw_data(n) {
            let v = row.get(col).copied().unwrap_or(0.0);
            if self.rising && v < self.last && (self.last - v) > 15.0 {
                let dt = self.last_beat.elapsed().as_secs_f64();
                if dt > 0.3 && dt < 2.0 {
                    self.bpm = 60.0 / dt;
                }
                self.last_beat = std::time::Instant::now();
                self.rising = false;
            } else if v > self.last {
                self.rising = true;
            }
            self.last = v;
            self.wave.push(v);
        }
        if self.wave.len() > 750 {
            let extra = self.wave.len() - 750;
            self.wave.drain(0..extra);
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        if !source.supports_aux_widgets() {
            ui.label("no aux — Pulse Sensor is Cyton analog only (not faked on Synthetic)");
            return;
        }
        if source.analog_channels().is_empty() {
            ui.label("no aux");
            return;
        }
        if source.cyton_board_mode() != Some(2) && ui.button("Turn Analog Read On").clicked() {
            self.pending_mode = Some(2);
        }
        ui.label(format!("BPM {:.0}", self.bpm));
        let plot = lock_plot_interaction(Plot::new("pulse_plot").height(120.0));
        plot.show(ui, |plot_ui| {
            let pts: PlotPoints = self
                .wave
                .iter()
                .enumerate()
                .map(|(i, v)| [i as f64, *v])
                .collect();
            plot_ui.line(Line::new("pulse", pts));
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
