//! W_BandPower — Java `W_BandPower.pde` histogram of EEG band PSD.

use crate::board::DataSource;
use crate::fft::{mean_band_powers, BAND_PLOT_LABELS};
use crate::laterality::latch_rails;
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use egui_plot::{Bar, BarChart, Plot};

/// Java `bp_plot.setYLim(0.1, 100)` — locked. Muscle/gamma may clip; they must not resize the chart.
const LOG_YMIN: f64 = -1.0; // log10(0.1)
const JAVA_YMAX: f64 = 100.0;

fn log_y_max(_powers: &[f64; 5]) -> f64 {
    JAVA_YMAX.log10()
}

pub struct WBandPower {
    title: String,
    smoothed_powers: [f64; 5],
    selected_channel: Option<usize>, // None = average selected (all) channels
    smoothing_index: usize,
    window_sec: f32,
    railed: [bool; 8],
}

impl WBandPower {
    pub fn new() -> Self {
        Self {
            title: "Band Power".to_string(),
            smoothed_powers: [0.0; 5],
            selected_channel: None,
            smoothing_index: 2, // 0.75 — same display Smooth as FFT
            window_sec: 5.0,
            railed: [false; 8],
        }
    }

    #[allow(dead_code)]
    pub fn set_smoothing_index(&mut self, index: usize) {
        self.smoothing_index = index.min(crate::widgets::SMOOTH_FACTORS.len() - 1);
    }

    pub fn smoothing_index(&self) -> usize {
        self.smoothing_index
    }

    pub fn set_window_sec(&mut self, seconds: f32) {
        self.window_sec = seconds.max(1.0);
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
        let sr = source.sample_rate();
        let window_size = ((self.window_sec as f64) * sr as f64)
            .round()
            .max(crate::fft::nfft_safe(sr) as f64) as usize;
        let data = source.get_data(window_size);
        if data.is_empty() {
            return;
        }
        let exg = source.exg_channels();
        if exg.is_empty() {
            return;
        }

        let raw_rows = source.get_raw_data(window_size.max(32));
        let mut raw_chs: Vec<Vec<f64>> = Vec::new();
        for &board_ch in exg.iter().take(8) {
            raw_chs.push(
                raw_rows
                    .iter()
                    .map(|row| row.get(board_ch).copied().unwrap_or(0.0))
                    .collect(),
            );
        }
        latch_rails(&mut self.railed, &raw_chs);

        let mut cols: Vec<Vec<f64>> = Vec::new();
        if let Some(logical) = self.selected_channel {
            if logical < 8 && self.railed[logical] {
                return;
            }
            if let Some(&board_ch) = exg.get(logical) {
                cols.push(
                    data.iter()
                        .map(|row| row.get(board_ch).copied().unwrap_or(0.0))
                        .collect(),
                );
            }
        } else {
            for (i, &board_ch) in exg.iter().enumerate() {
                if i < 8 && self.railed[i] {
                    continue;
                }
                cols.push(
                    data.iter()
                        .map(|row| row.get(board_ch).copied().unwrap_or(0.0))
                        .collect(),
                );
            }
        }
        if cols.is_empty() {
            return;
        }
        let raw = mean_band_powers(&cols, source.sample_rate() as f64);
        let factor = crate::widgets::SMOOTH_FACTORS
            .get(self.smoothing_index)
            .copied()
            .unwrap_or(0.75) as f64;
        for (s, &r) in self.smoothed_powers.iter_mut().zip(raw.iter()) {
            *s = *s * factor + r * (1.0 - factor);
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
                    .fill(egui::Color32::from_rgba_unmultiplied(
                        c.r(),
                        c.g(),
                        c.b(),
                        200,
                    ))
                    .width(0.8)
            })
            .collect();

        let plot_h = (ui.available_height() - 16.0).max(80.0);
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
    use super::{log_y_max, WBandPower, JAVA_YMAX};

    #[test]
    fn modest_powers_keep_java_ymax_of_100() {
        assert!((10f64.powf(log_y_max(&[10.0, 8.0, 20.0, 5.0, 2.0])) - JAVA_YMAX).abs() < 1e-9);
    }

    #[test]
    fn large_powers_clip_instead_of_raising_the_ceiling() {
        let ymax = 10f64.powf(log_y_max(&[400.0, 350.0, 500.0, 200.0, 100.0]));
        assert!((ymax - JAVA_YMAX).abs() < 1e-9);
    }

    #[test]
    fn default_smoothing_follows_top_bar_075() {
        assert_eq!(WBandPower::new().smoothing_index(), 2);
        assert!((crate::widgets::SMOOTH_FACTORS[2] - 0.75).abs() < 1e-6);
    }
}
