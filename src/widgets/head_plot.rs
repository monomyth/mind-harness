//! WHeadPlot — topographic 2D head map (the iconic "Head Plot" widget from the original Java GUI).
//!
//! Simple but recognizable implementation:
//! - Circular head outline with ears/nose hints
//! - 8 electrode positions (10-20 style layout for Cyton 8ch; extra channels reuse)
//! - Color + size mapped to smoothed channel power (RMS of recent abs values)
//! - Auto-scaled colormap (blue=low → cyan/green → yellow/red=high)
//! - Works identically in live, Synthetic, and Playback modes via DataSource.

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;

pub struct WHeadPlot {
    title: String,
    /// Smoothed power (abs-mean) per channel for stable coloring
    smoothed: Vec<f32>,
    smooth_factor: f32,
}

impl WHeadPlot {
    pub fn new() -> Self {
        Self {
            title: "Head Plot".to_string(),
            smoothed: vec![0.0; 16],
            smooth_factor: 0.75, // a bit of temporal smoothing for nice visuals
        }
    }
}

impl Default for WHeadPlot {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WHeadPlot {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        // Use a small recent window for power estimation (cheap & responsive)
        let data = source.get_data(40);
        let exg = source.exg_channels();
        // Only ever support 8 electrodes visually (Issue 4). Higher channels on 16ch boards are ignored
        // for the topographic view (we keep the visual simple and recognizable).
        for (i, &ch) in exg.iter().enumerate().take(8) {
            if i >= self.smoothed.len() {
                break;
            }
            let mut sum = 0.0f32;
            let mut cnt = 0usize;
            for row in &data {
                if ch < row.len() {
                    sum += row[ch].abs() as f32;
                    cnt += 1;
                }
            }
            let val = if cnt > 0 { sum / cnt as f32 } else { 0.0 };
            self.smoothed[i] =
                self.smooth_factor * self.smoothed[i] + (1.0 - self.smooth_factor) * val;
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let exg = source.exg_channels();
        let n = exg.len().min(16);

        ui.horizontal(|ui| {
            ui.label("Head Plot — Relative Power");
            if ui.small_button("Reset").clicked() {
                self.smoothed.fill(0.0);
            }
            if n > 8 {
                ui.colored_label(egui::Color32::from_rgb(200, 160, 60), "(ch 1-8 only)");
            } else {
                ui.small(format!("({} ch)", n));
            }
        });
        ui.small("Color & size = smoothed power (blue=low → red=high) • labels ≈ 10-20 positions");
        ui.add_space(2.0);

        // Allocate drawing canvas (fixed reasonable height so it plays nice in scrollable side panel)
        let desired = egui::vec2(ui.available_width().max(200.0), 210.0);
        let (resp, painter) = ui.allocate_painter(desired, egui::Sense::hover());
        let rect = resp.rect;
        let center = rect.center();
        let radius = (rect.width().min(rect.height()) * 0.40).min(85.0);

        // Head (skin)
        painter.circle_filled(center, radius, egui::Color32::from_rgb(255, 218, 195));
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(140, 90, 70)),
        );

        // Ears
        let ear_off = radius * 0.92;
        let ear_r = radius * 0.16;
        painter.circle_filled(
            center + egui::vec2(-ear_off, 0.0),
            ear_r,
            egui::Color32::from_rgb(255, 208, 185),
        );
        painter.circle_filled(
            center + egui::vec2(ear_off, 0.0),
            ear_r,
            egui::Color32::from_rgb(255, 208, 185),
        );

        // Nose hint (downward triangle at top of head)
        let nose_tip = center + egui::vec2(0.0, -radius * 0.82);
        let nose_w = 5.0;
        painter.line_segment(
            [
                nose_tip + egui::vec2(-nose_w, 6.0),
                nose_tip + egui::vec2(0.0, -8.0),
            ],
            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(170, 110, 90)),
        );
        painter.line_segment(
            [
                nose_tip + egui::vec2(nose_w, 6.0),
                nose_tip + egui::vec2(0.0, -8.0),
            ],
            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(170, 110, 90)),
        );

        // Classic 8-channel electrode positions (normalized -1..+1, y positive = down in screen)
        // Rough 10-20 inspired placement for visual recognition (not exact medical coords)
        let electrode_positions: [(f32, f32); 8] = [
            (-0.28, -0.52), // 1 Fp1-ish
            (0.28, -0.52),  // 2 Fp2
            (-0.58, -0.08), // 3 F7 / C3 area
            (0.58, -0.08),  // 4 F8
            (-0.42, 0.22),  // 5 C3 / T7
            (0.42, 0.22),   // 6 C4 / T8
            (-0.22, 0.58),  // 7 P3 / O1-ish
            (0.22, 0.58),   // 8 P4 / O2-ish
        ];

        // Auto-scale to current max power (prevents needing a manual "gain" slider)
        let max_amp = self
            .smoothed
            .iter()
            .take(n)
            .fold(1.0_f32, |m, &v| m.max(v.abs()));

        for (i, &(nx, ny)) in electrode_positions.iter().enumerate().take(8) {
            let pos = center + egui::vec2(nx * radius, ny * radius);
            let amp = self.smoothed.get(i).copied().unwrap_or(0.0);
            let norm = if max_amp > 0.001 {
                (amp / max_amp).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Simple multi-stop "jet-ish" colormap.
            // The if-arms + the norm clamp(0,1) guarantee every intermediate value is ≤ 255,
            // so the `as u8` casts are safe (the previous .min(255) triggered an "unnecessary_min" lint).
            // If the stop points ever change, re-audit the expressions.
            let color = if norm < 0.2 {
                egui::Color32::from_rgb(30, (norm * 5.0 * 200.0) as u8 + 55, 220)
            } else if norm < 0.4 {
                egui::Color32::from_rgb(0, 200, ((0.4 - norm) * 5.0 * 200.0) as u8 + 30)
            } else if norm < 0.6 {
                egui::Color32::from_rgb(((norm - 0.4) * 5.0 * 220.0) as u8, 230, 40)
            } else if norm < 0.8 {
                egui::Color32::from_rgb(230, ((0.8 - norm) * 5.0 * 200.0) as u8 + 30, 20)
            } else {
                egui::Color32::from_rgb(240, 50 + ((1.0 - norm) * 80.0) as u8, 10)
            };

            let r = 7.0 + norm * 7.0; // bigger = stronger
            painter.circle_filled(pos, r, color);
            painter.circle_stroke(
                pos,
                r + 0.5,
                egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(180)),
            );

            // More recognizable labels (approximate 10-20 positions)
            let label = ["Fp1", "Fp2", "F7", "F8", "C3", "C4", "P3", "P4"][i];
            painter.text(
                pos + egui::vec2(0.0, r + 6.0),
                egui::Align2::CENTER_TOP,
                label,
                egui::FontId::proportional(6.5),
                egui::Color32::from_gray(60),
            );
        }

        // Color legend bar at the bottom (much more descriptive)
        let bar_y = rect.max.y - 14.0;
        let bar_left = rect.min.x + 12.0;
        let bar_right = rect.max.x - 12.0;
        let bar_w = bar_right - bar_left;

        // Draw a simple 5-segment gradient approximating the jet-ish colormap
        let stops = [
            (0.0, egui::Color32::from_rgb(30, 75, 220)),
            (0.25, egui::Color32::from_rgb(0, 200, 150)),
            (0.5, egui::Color32::from_rgb(120, 230, 40)),
            (0.75, egui::Color32::from_rgb(230, 140, 20)),
            (1.0, egui::Color32::from_rgb(240, 60, 10)),
        ];

        for i in 0..4 {
            let x1 = bar_left + bar_w * stops[i].0;
            let x2 = bar_left + bar_w * stops[i + 1].0;
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x1, bar_y - 4.0), egui::pos2(x2, bar_y + 4.0)),
                0.0,
                stops[i].1,
            );
        }

        painter.text(
            egui::pos2(bar_left, bar_y + 10.0),
            egui::Align2::LEFT_CENTER,
            "low power",
            egui::FontId::proportional(8.0),
            egui::Color32::from_gray(120),
        );
        painter.text(
            egui::pos2(bar_right, bar_y + 10.0),
            egui::Align2::RIGHT_CENTER,
            "high power",
            egui::FontId::proportional(8.0),
            egui::Color32::from_gray(120),
        );
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
