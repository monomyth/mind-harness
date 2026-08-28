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
    max_points: usize,
}

impl WAccelerometer {
    pub fn new() -> Self {
        Self {
            title: "Accelerometer".to_string(),
            history: Vec::new(),
            max_points: 500,
        }
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
        for row in latest {
            let x = row.get(accel_chans[0]).copied().unwrap_or(0.0);
            let y = row.get(accel_chans[1]).copied().unwrap_or(0.0);
            let z = row.get(accel_chans[2]).copied().unwrap_or(0.0);
            self.history.push((x, y, z));
        }
        if self.history.len() > self.max_points {
            let extra = self.history.len() - self.max_points;
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
                plot_ui.line(Line::new("X", x_points).color(crate::theme::ACCEL_X).width(1.2_f32));
                plot_ui.line(Line::new("Y", y_points).color(crate::theme::ACCEL_Y).width(1.2_f32));
                plot_ui.line(Line::new("Z", z_points).color(crate::theme::ACCEL_Z).width(1.2_f32));
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
