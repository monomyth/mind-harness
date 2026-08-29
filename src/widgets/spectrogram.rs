//! WSpectrogram — scrolling frequency-over-time heatmap (port of W_Spectrogram.pde).

use crate::board::DataSource;
use crate::fft::compute_fft_magnitude;
use crate::widgets::Widget;
use eframe::egui;
use std::collections::VecDeque;

const BINS: usize = 48;
const MAX_COLS: usize = 180;

pub struct WSpectrogram {
    title: String,
    channel: usize,
    columns: VecDeque<Vec<f32>>,
}

impl WSpectrogram {
    pub fn new() -> Self {
        Self {
            title: "Spectrogram".to_string(),
            channel: 0,
            columns: VecDeque::with_capacity(MAX_COLS),
        }
    }
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
        let data = source.get_data(256);
        if data.len() < 32 {
            return;
        }
        let samples: Vec<f64> = data
            .iter()
            .map(|row| row.get(ch).copied().unwrap_or(0.0))
            .collect();
        let (_freqs, mags) = compute_fft_magnitude(&samples, source.sample_rate() as f64, 60.0);
        if mags.is_empty() {
            return;
        }
        let mut col = vec![0.0f32; BINS];
        let step = (mags.len() as f32 / BINS as f32).max(1.0);
        for (i, slot) in col.iter_mut().enumerate() {
            let start = (i as f32 * step) as usize;
            let end = ((i as f32 + 1.0) * step) as usize;
            let slice =
                &mags[start.min(mags.len())..end.min(mags.len()).max(start + 1).min(mags.len())];
            if !slice.is_empty() {
                *slot = slice.iter().copied().fold(f64::NEG_INFINITY, f64::max) as f32;
            }
        }
        self.columns.push_back(col);
        while self.columns.len() > MAX_COLS {
            self.columns.pop_front();
        }
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
                        }
                    }
                });
            ui.small("0–60 Hz  •  newest on the right");
        });

        let desired = egui::vec2(ui.available_width(), ui.available_height().max(80.0));
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
