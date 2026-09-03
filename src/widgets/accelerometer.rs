//! W_Accelerometer — 3-axis accelerometer visualization.
//!
//! Port of the original W_Accelerometer.pde. Shows X/Y/Z acceleration.

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

pub struct WAccelerometer {
    title: String,
    history: Vec<(f64, f64, f64)>, // (x, y, z)
    window_sec: f32,
    smoothing_index: usize,
    last: Option<(f64, f64, f64)>,
}

impl WAccelerometer {
    pub fn new() -> Self {
        Self {
            title: "Accelerometer".to_string(),
            history: Vec::new(),
            window_sec: 5.0,
            smoothing_index: 2,
            last: None,
        }
    }

    pub fn set_window_sec(&mut self, seconds: f32) {
        self.window_sec = seconds.max(1.0);
    }

    pub fn set_smoothing_index(&mut self, index: usize) {
        self.smoothing_index = index.min(crate::widgets::SMOOTH_FACTORS.len() - 1);
    }
}

impl Default for WAccelerometer {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WAccelerometer {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let accel_chans = source.accel_channels();
        if accel_chans.len() < 3 {
            return;
        }

        let n = source.recent_samples_delivered();
        if n == 0 {
            return;
        }
        let latest = source.get_data(n);
        let factor = crate::widgets::SMOOTH_FACTORS
            .get(self.smoothing_index)
            .copied()
            .unwrap_or(0.0) as f64;
        for row in latest {
            let mut x = row.get(accel_chans[0]).copied().unwrap_or(0.0);
            let mut y = row.get(accel_chans[1]).copied().unwrap_or(0.0);
            let mut z = row.get(accel_chans[2]).copied().unwrap_or(0.0);
            if factor > 0.0 {
                if let Some((px, py, pz)) = self.last {
                    x = px * factor + x * (1.0 - factor);
                    y = py * factor + y * (1.0 - factor);
                    z = pz * factor + z * (1.0 - factor);
                }
            }
            self.last = Some((x, y, z));
            self.history.push((x, y, z));
        }
        let keep = ((self.window_sec as f64) * source.sample_rate() as f64)
            .round()
            .max(10.0) as usize;
        if self.history.len() > keep {
            let extra = self.history.len() - keep;
            self.history.drain(0..extra);
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let accel_chans = source.accel_channels();

        if accel_chans.len() < 3 {
            ui.label("Accelerometer data not available for this board.");
            return;
        }

        let x_points: PlotPoints = self
            .history
            .iter()
            .enumerate()
            .map(|(i, (x, _, _))| [i as f64, *x])
            .collect();
        let y_points: PlotPoints = self
            .history
            .iter()
            .enumerate()
            .map(|(i, (_, y, _))| [i as f64, *y])
            .collect();
        let z_points: PlotPoints = self
            .history
            .iter()
            .enumerate()
            .map(|(i, (_, _, z))| [i as f64, *z])
            .collect();

        crate::widgets::lock_plot_interaction(Plot::new("accel_plot"))
            .height(ui.available_height())
            .show_axes([true, true])
            .y_axis_min_width(40.0)
            .x_axis_formatter(|mark, _| crate::widgets::axis_tick_label(mark.value))
            .y_axis_formatter(|mark, _| crate::widgets::axis_tick_label(mark.value))
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("X", x_points)
                        .color(crate::theme::ACCEL_X)
                        .width(1.2_f32),
                );
                plot_ui.line(
                    Line::new("Y", y_points)
                        .color(crate::theme::ACCEL_Y)
                        .width(1.2_f32),
                );
                plot_ui.line(
                    Line::new("Z", z_points)
                        .color(crate::theme::ACCEL_Z)
                        .width(1.2_f32),
                );
            });

        ui.small("Accel X/Y/Z (g)");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
