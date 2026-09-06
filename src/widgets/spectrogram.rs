//! WSpectrogram — scrolling frequency-over-time heatmap (port of W_Spectrogram.pde).
//!
//! Performance: Implements incremental updates - only computes FFT for new columns
//! rather than rebuilding the entire spectrogram every frame.

use crate::board::DataSource;
use crate::fft::compute_fft_magnitude;
use crate::widgets::Widget;
use eframe::egui;
use std::collections::VecDeque;

const BINS: usize = 48;
const COLS_PER_SEC: f32 = 20.0;

pub struct WSpectrogram {
    title: String,
    channel: usize,
    columns: VecDeque<Vec<f32>>,
    window_sec: f32,
    smoothing_index: usize,
    last_sample_count: usize,
    last_hop: usize,
}

impl WSpectrogram {
    pub fn new() -> Self {
        Self {
            title: "Spectrogram".to_string(),
            channel: 0,
            columns: VecDeque::new(),
            window_sec: 5.0,
            smoothing_index: 2,
            last_sample_count: 0,
            last_hop: 0,
        }
    }

    pub fn set_window_sec(&mut self, seconds: f32) {
        self.window_sec = seconds.max(1.0);
    }

    pub fn set_smoothing_index(&mut self, index: usize) {
        self.smoothing_index = index.min(crate::widgets::SMOOTH_FACTORS.len() - 1);
    }

    pub fn window_sec(&self) -> f32 {
        self.window_sec
    }
}

/// Columns spanning `window_sec`, hop so newest is last. `nfft` is the FFT slice.
pub fn spectrogram_starts(n_samples: usize, nfft: usize, hop: usize) -> Vec<usize> {
    if n_samples < nfft || hop == 0 {
        return Vec::new();
    }
    let extra = (n_samples - nfft) % hop;
    let mut starts = Vec::new();
    let mut s = extra;
    while s + nfft <= n_samples {
        starts.push(s);
        s += hop;
    }
    starts
}

impl Default for WSpectrogram {
    fn default() -> Self {
        Self::new()
    }
}

fn mag_to_color(t: f32) -> egui::Color32 {
    // Blue → cyan → green → yellow → red, similar to the Java hue sweep.
    let t = t.clamp(0.0, 1.0);
    if t < 0.25 {
        let u = t / 0.25;
        egui::Color32::from_rgb(0, (u * 180.0) as u8, 200)
    } else if t < 0.5 {
        let u = (t - 0.25) / 0.25;
        egui::Color32::from_rgb(0, 180 + (u * 75.0) as u8, 200 - (u * 200.0) as u8)
    } else if t < 0.75 {
        let u = (t - 0.5) / 0.25;
        egui::Color32::from_rgb((u * 255.0) as u8, 255, 0)
    } else {
        let u = (t - 0.75) / 0.25;
        egui::Color32::from_rgb(255, 255 - (u * 255.0) as u8, 0)
    }
}

impl Widget for WSpectrogram {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let exg = source.exg_channels();
        if exg.is_empty() {
            return;
        }
        self.channel = self.channel.min(exg.len() - 1);
        let ch = exg[self.channel];
        let sr = source.sample_rate() as f64;
        let n = ((self.window_sec as f64) * sr).round() as usize;
        let samples = source.get_channel_data(ch, n.max(32));
        if samples.len() < 32 {
            return;
        }
        let nfft = 256.min(samples.len());
        let hop = ((sr / COLS_PER_SEC as f64).round() as usize).max(1);
        let starts = spectrogram_starts(samples.len(), nfft, hop);
        let factor = crate::widgets::SMOOTH_FACTORS
            .get(self.smoothing_index)
            .copied()
            .unwrap_or(0.0) as f64;

        let current_sample_count = samples.len();
        let new_samples = current_sample_count.saturating_sub(self.last_sample_count);
        
        let can_use_incremental = !self.columns.is_empty()
            && self.last_hop == hop
            && new_samples > 0
            && new_samples < current_sample_count / 2;
        
        if can_use_incremental {
            let new_cols_needed = (new_samples + hop - 1) / hop;
            let total_cols = starts.len();
            
            while self.columns.len() > total_cols.saturating_sub(new_cols_needed) {
                self.columns.pop_front();
            }
            
            let start_col = self.columns.len();
            for (ci, start) in starts.iter().enumerate().skip(start_col) {
                let slice = &samples[*start..*start + nfft];
                let (_freqs, mags) = compute_fft_magnitude(slice, sr, 60.0);
                if mags.is_empty() {
                    continue;
                }
                let mut col = vec![0.0f32; BINS];
                let step = (mags.len() as f32 / BINS as f32).max(1.0);
                for (i, slot) in col.iter_mut().enumerate() {
                    let a = (i as f32 * step) as usize;
                    let b = ((i as f32 + 1.0) * step) as usize;
                    let slice =
                        &mags[a.min(mags.len())..b.min(mags.len()).max(a + 1).min(mags.len())];
                    if !slice.is_empty() {
                        *slot = slice.iter().copied().fold(f64::NEG_INFINITY, f64::max) as f32;
                    }
                }
                if factor > 0.0 {
                    if let Some(prev) = self.columns.get(ci) {
                        if prev.len() == col.len() {
                            for (n, o) in col.iter_mut().zip(prev.iter()) {
                                *n = (*o as f64 * factor + *n as f64 * (1.0 - factor)) as f32;
                            }
                        }
                    }
                }
                self.columns.push_back(col);
            }
        } else {
            let mut new_cols: VecDeque<Vec<f32>> = VecDeque::new();
            for (ci, start) in starts.iter().enumerate() {
                let slice = &samples[*start..*start + nfft];
                let (_freqs, mags) = compute_fft_magnitude(slice, sr, 60.0);
                if mags.is_empty() {
                    continue;
                }
                let mut col = vec![0.0f32; BINS];
                let step = (mags.len() as f32 / BINS as f32).max(1.0);
                for (i, slot) in col.iter_mut().enumerate() {
                    let a = (i as f32 * step) as usize;
                    let b = ((i as f32 + 1.0) * step) as usize;
                    let slice =
                        &mags[a.min(mags.len())..b.min(mags.len()).max(a + 1).min(mags.len())];
                    if !slice.is_empty() {
                        *slot = slice.iter().copied().fold(f64::NEG_INFINITY, f64::max) as f32;
                    }
                }
                if factor > 0.0 {
                    if let Some(prev) = self.columns.get(ci) {
                        if prev.len() == col.len() {
                            for (n, o) in col.iter_mut().zip(prev.iter()) {
                                *n = (*o as f64 * factor + *n as f64 * (1.0 - factor)) as f32;
                            }
                        }
                    }
                }
                new_cols.push_back(col);
            }
            self.columns = new_cols;
        }
        
        self.last_sample_count = current_sample_count;
        self.last_hop = hop;
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let n = source.exg_channels().len().max(1);
        ui.horizontal(|ui| {
            ui.label("Channel");
            egui::ComboBox::from_id_salt("spect_ch")
                .selected_text(format!("Ch {}", self.channel + 1))
                .show_ui(ui, |ui| {
                    for i in 0..n {
                        if ui
                            .selectable_label(self.channel == i, format!("Ch {}", i + 1))
                            .clicked()
                        {
                            self.channel = i;
                            self.columns.clear();
                            self.last_sample_count = 0;
                        }
                    }
                });
            ui.small(format!(
                "0–60 Hz  •  {:.0}s  •  newest on the right",
                self.window_sec
            ));
        });

        let desired = egui::vec2(ui.available_width(), (ui.available_height() - 8.0).max(80.0));
        let (resp, painter) = ui.allocate_painter(desired, egui::Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(10, 16, 28));

        if self.columns.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for data…",
                egui::FontId::proportional(13.0),
                egui::Color32::from_gray(180),
            );
            return;
        }

        let cols = self.columns.len() as f32;
        let col_w = rect.width() / cols.max(1.0);
        let bin_h = rect.height() / BINS as f32;

        // Normalize colour by the current window's max so the map stays readable.
        let mut max_v = -80.0f32;
        for col in &self.columns {
            for &v in col {
                if v > max_v {
                    max_v = v;
                }
            }
        }
        let min_v = max_v - 40.0;

        for (ci, col) in self.columns.iter().enumerate() {
            let x = rect.left() + ci as f32 * col_w;
            for (bi, &v) in col.iter().enumerate() {
                // Low frequency at the bottom, like the Java widget.
                let y = rect.bottom() - (bi as f32 + 1.0) * bin_h;
                let t = ((v - min_v) / (max_v - min_v + 1e-3)).clamp(0.0, 1.0);
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(col_w.ceil(), bin_h.ceil()),
                    ),
                    0.0,
                    mag_to_color(t),
                );
            }
        }
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
    use super::*;

    #[test]
    fn columns_span_the_window_and_newest_is_last() {
        let sr = 250.0_f64;
        let window = 5.0_f32;
        let n = (window as f64 * sr) as usize;
        let nfft = 256;
        let hop = (sr / COLS_PER_SEC as f64).round() as usize;
        let starts = spectrogram_starts(n, nfft, hop);
        assert!(!starts.is_empty());
        let last_end = starts.last().copied().unwrap() + nfft;
        assert_eq!(last_end, n, "newest column ends at the newest sample");
        let span = (n - starts[0]) as f32 / sr as f32;
        assert!(
            (span - window).abs() < 1.1,
            "span {span}s should be the window {window}s"
        );
        assert!(starts.windows(2).all(|w| w[1] > w[0]));
    }
}
