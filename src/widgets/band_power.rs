//! W_BandPower — Java `W_BandPower.pde` histogram of EEG band PSD.

use crate::board::DataSource;
use crate::fft::{mean_band_powers, BAND_PLOT_LABELS};
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use egui_plot::{Bar, BarChart, Plot};

/// Java `bp_plot.setYLim(0.1, 100)` — keep 0.1 as the floor; raise the ceiling when data exceeds 100.
const LOG_YMIN: f64 = -1.0; // log10(0.1)
const JAVA_YMAX: f64 = 100.0;

fn log_y_max(powers: &[f64; 5]) -> f64 {
    let peak = powers.iter().copied().fold(0.1_f64, f64::max);
    (peak * 2.0).max(JAVA_YMAX).log10()
}

pub struct WBandPower {
    title: String,
    smoothed_powers: [f64; 5],
    selected_channel: Option<usize>, // None = average selected (all) channels
    smoothing_index: usize,
}

impl WBandPower {
    pub fn new() -> Self {
        Self {
            title: "Band Power".to_string(),
            smoothed_powers: [0.0; 5],
            selected_channel: None,
            smoothing_index: 2,
        }
    }

    #[allow(dead_code)]
    pub fn set_smoothing_index(&mut self, index: usize) {
        self.smoothing_index = index.min(crate::widgets::SMOOTH_FACTORS.len() - 1);
    }

    pub fn smoothing_index(&self) -> usize {
        self.smoothing_index
    }
}

impl Default for WBandPower {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WBandPower {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let window_size = crate::fft::nfft_safe(source.sample_rate());
        let data = source.get_data(window_size);
        if data.is_empty() {
            return;
        }
        let exg = source.exg_channels();
        if exg.is_empty() {
            return;
        }

        let mut cols: Vec<Vec<f64>> = Vec::new();
        if let Some(logical) = self.selected_channel {
            if let Some(&board_ch) = exg.get(logical) {
                cols.push(
                    data.iter()
                        .map(|row| row.get(board_ch).copied().unwrap_or(0.0))
                        .collect(),
                );
            }
        } else {
            for &board_ch in exg {
                cols.push(
                    data.iter()
                        .map(|row| row.get(board_ch).copied().unwrap_or(0.0))
                        .collect(),
                );
            }
        }
        let raw = mean_band_powers(&cols, source.sample_rate() as f64);
        let factor = crate::widgets::SMOOTH_FACTORS
            .get(self.smoothing_index)
            .copied()
            .unwrap_or(0.75) as f64;
        for (s, &r) in self.smoothed_powers.iter_mut().zip(raw.iter()) {
            *s = *s * factor as f64 + r * (1.0 - factor as f64);
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let exg = source.exg_channels();
        let num_chans = exg.len();
        // Java histogram fill: channelColors[6,4,3,2,1]
        let colors = [
            theme::channel_color(6),
            theme::channel_color(4),
            theme::channel_color(3),
            theme::channel_color(2),
            theme::channel_color(1),
        ];

        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("bp_view")
                .selected_text(match self.selected_channel {
                    None => "All channels".to_string(),
                    Some(ch) => format!("Channel {}", ch + 1),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.selected_channel.is_none(), "All channels")
                        .clicked()
                    {
                        self.selected_channel = None;
                    }
                    for ch in 0..num_chans {
                        if ui
                            .selectable_label(
                                self.selected_channel == Some(ch),
                                format!("Channel {}", ch + 1),
                            )
                            .clicked()
                        {
                            self.selected_channel = Some(ch);
                        }
                    }
                });

            let labels = ["0.0", "0.5", "0.75", "0.9", "0.95", "0.98", "0.99", "0.999"];
            let current_label = labels.get(self.smoothing_index).copied().unwrap_or("0.75");
            ui.label("Smooth");
            egui::ComboBox::from_id_salt("bp_smooth")
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    for (i, &lab) in labels.iter().enumerate() {
                        if ui
                            .selectable_label(self.smoothing_index == i, lab)
                            .clicked()
                        {
                            self.smoothing_index = i;
                        }
                    }
                });
        });

        let log_ymax = log_y_max(&self.smoothed_powers);
        let bars: Vec<Bar> = self
            .smoothed_powers
            .iter()
            .enumerate()
            .map(|(i, &power)| {
                let log_p = power.max(0.1).log10().min(log_ymax);
                let c = colors[i];
                Bar::new(i as f64, log_p - LOG_YMIN)
                    .base_offset(LOG_YMIN)
                    .name(BAND_PLOT_LABELS[i])
                    .fill(egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 200))
                    .width(0.8)
            })
            .collect();

        let plot_h = ui.available_height().max(80.0);
        crate::widgets::lock_plot_interaction(Plot::new("band_power_plot"))
            .height(plot_h)
            .allow_boxed_zoom(false)
            .show_axes([true, true])
            .show_grid([false, true])
            .auto_bounds([false, false])
            .default_x_bounds(-0.5, 4.5)
            .default_y_bounds(LOG_YMIN, log_ymax)
            .y_axis_min_width(44.0)
            .x_axis_label("EEG Power Bands")
            .y_axis_label("Power — (uV)^2 / Hz")
            .x_axis_formatter(|mark, _| {
                let names = ["DELTA", "THETA", "ALPHA", "BETA", "GAMMA"];
                let i = mark.value.round() as i32;
                if (mark.value - i as f64).abs() < 0.15 && (0..5).contains(&i) {
                    names[i as usize].to_string()
                } else {
                    String::new()
                }
            })
            .y_axis_formatter(|mark, _| {
                if (mark.value - mark.value.round()).abs() < 0.05 {
                    let v = 10f64.powf(mark.value.round());
                    if (v - 0.1).abs() < 0.02 {
                        "0.1".into()
                    } else {
                        format!("{:.0}", v)
                    }
                } else {
                    String::new()
                }
            })
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(BarChart::new("bands", bars));
            });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{log_y_max, JAVA_YMAX};

    #[test]
    fn modest_powers_keep_java_ymax_of_100() {
        assert!((10f64.powf(log_y_max(&[10.0, 8.0, 20.0, 5.0, 2.0])) - JAVA_YMAX).abs() < 1e-9);
    }

    #[test]
    fn large_powers_raise_the_ceiling_instead_of_clipping() {
        let ymax = 10f64.powf(log_y_max(&[400.0, 350.0, 500.0, 200.0, 100.0]));
        assert!(ymax > JAVA_YMAX);
        assert!(ymax >= 1000.0);
    }
}
