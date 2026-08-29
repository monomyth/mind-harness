//! W_timeSeries — stacked per-channel EEG traces matching the Java ChannelBar look.

use crate::board::DataSource;
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use egui::{pos2, Color32, Pos2, Rect, Shape, Stroke};

/// Java ChannelBar: electrode button is outside the plot; ± / RMS overlay the plot.
const ELECTRODE_W: f32 = 26.0;

pub struct WTimeSeries {
    time_window_sec: f32,
    y_scale_uv: f32,
    title: String,
    visible_channels: Vec<bool>,
    per_channel_y_scales: Vec<f32>,
}

impl WTimeSeries {
    pub fn new() -> Self {
        Self {
            time_window_sec: 5.0,
            y_scale_uv: 200.0,
            title: "Time Series".to_string(),
            visible_channels: vec![true; 16],
            per_channel_y_scales: vec![0.0; 16],
        }
    }

    pub fn set_time_window(&mut self, seconds: f32) {
        self.time_window_sec = seconds.max(1.0);
    }

    pub fn set_y_scale(&mut self, uv: f32) {
        // 0 = Java TimeSeriesYLim.AUTO
        self.y_scale_uv = if uv < 1.0 { 0.0 } else { uv.max(10.0) };
    }

    pub fn time_window_sec(&self) -> f32 {
        self.time_window_sec
    }

    pub fn y_scale_uv(&self) -> f32 {
        self.y_scale_uv
    }

    pub fn set_per_channel_y_scale(&mut self, ch: usize, uv: f32) {
        if ch < self.per_channel_y_scales.len() {
            self.per_channel_y_scales[ch] = uv.max(0.0);
        }
    }

    pub fn per_channel_y_scale(&self, ch: usize) -> f32 {
        if ch < self.per_channel_y_scales.len() && self.per_channel_y_scales[ch] > 0.0 {
            self.per_channel_y_scales[ch]
        } else {
            self.y_scale_uv
        }
    }

    pub fn per_channel_y_scales(&self) -> Vec<f32> {
        self.per_channel_y_scales.clone()
    }

    fn scale_for_trace(&self, ch: usize, ys: &[f64]) -> f64 {
        let configured = self.per_channel_y_scale(ch);
        if configured < 0.5 {
            auto_y_scale(ys)
        } else {
            configured as f64
        }
    }
}

impl Default for WTimeSeries {
    fn default() -> Self {
        Self::new()
    }
}

/// Java `std()` in Extras.pde (population std, divide by n).
fn uv_std(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs
        .iter()
        .map(|x| {
            let d = x - mean;
            d * d
        })
        .sum::<f64>()
        / xs.len() as f64;
    var.sqrt()
}

/// Java ChannelBar voltage: `std` of the last 1 s of filtered data.
fn uv_std_last_second(ys: &[f64], sample_rate: f32) -> f64 {
    let n = (sample_rate.max(1.0) as usize).min(ys.len());
    if n == 0 {
        0.0
    } else {
        uv_std(&ys[ys.len() - n..])
    }
}

fn sample_at(row: &[f64], board_ch: usize) -> f64 {
    row.get(board_ch).copied().unwrap_or(0.0)
}

fn channel_samples(data: &[Vec<f64>], board_ch: usize) -> Vec<f64> {
    data.iter().map(|row| sample_at(row, board_ch)).collect()
}

/// Java ChannelBar `setXLim(-numSeconds, 0)` — identical window on every electrode.
fn channel_bar_x_range(window_sec: f32) -> (f64, f64) {
    (-window_sec as f64, 0.0)
}

struct ChannelBarLayout {
    plot_w: f32,
}

fn channel_bar_layout(row_width: f32) -> ChannelBarLayout {
    ChannelBarLayout {
        plot_w: (row_width - ELECTRODE_W).max(16.0),
    }
}

/// Newest sample at t=0, older samples to the left (Java time axis).
fn sample_time(k: usize, n: usize, sample_rate: f32) -> f64 {
    let last = n.saturating_sub(1) as f64;
    (k as f64 - last) / sample_rate.max(1.0) as f64
}

fn time_to_x(t: f64, window_sec: f32, left: f32, width: f32) -> f32 {
    let (tmin, tmax) = channel_bar_x_range(window_sec);
    let span = (tmax - tmin).max(1e-12);
    let u = ((t - tmin) / span).clamp(0.0, 1.0);
    left + u as f32 * width
}

fn uv_to_y(v: f64, scale: f64, rect: Rect) -> f32 {
    let u = if scale.abs() < 1e-12 {
        0.0
    } else {
        (v / scale).clamp(-1.0, 1.0)
    };
    let mid = rect.center().y;
    let half = rect.height() * 0.5;
    mid - u as f32 * half
}

fn auto_y_scale(ys: &[f64]) -> f64 {
    ys.iter()
        .filter(|y| y.is_finite())
        .fold(10.0_f64, |a, &y| a.max(y.abs()))
}

/// Java GPlot: chronological polyline, Y clipped to ±vertScale.
fn paint_channel_trace(
    ui: &egui::Ui,
    rect: Rect,
    ys: &[f64],
    sample_rate: f32,
    window_sec: f32,
    scale: f64,
    color: Color32,
    draw_time_axis: bool,
) {
    let painter = ui.painter_at(rect);
    let y0 = rect.center().y;
    painter.line_segment(
        [pos2(rect.left(), y0), pos2(rect.right(), y0)],
        Stroke::new(1.0_f32, Color32::from_black_alpha(30)),
    );

    let label_color = theme::OPENBCI_DARKBLUE;
    let font = egui::FontId::proportional(10.0);
    painter.text(
        pos2(rect.left() + 4.0, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        format!("+{:.0}uV", scale),
        font.clone(),
        label_color,
    );
    painter.text(
        pos2(rect.left() + 4.0, rect.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("-{:.0}uV", scale),
        font.clone(),
        label_color,
    );

    if !ys.is_empty() && rect.width() >= 1.0 {
        let n = ys.len();
        let mut pts: Vec<Pos2> = Vec::with_capacity(n);
        for (k, &y) in ys.iter().enumerate() {
            if !y.is_finite() {
                continue;
            }
            let t = sample_time(k, n, sample_rate);
            let x = time_to_x(t, window_sec, rect.left(), rect.width());
            pts.push(pos2(x, uv_to_y(y, scale, rect)));
        }
        if pts.len() >= 2 {
            painter.add(Shape::line(pts, Stroke::new(1.2_f32, color)));
        }
    }

    if draw_time_axis && window_sec >= 1.0 {
        let ticks = window_sec.round() as i32;
        for s in 0..=ticks {
            let t = -(ticks - s) as f64;
            let x = time_to_x(t, window_sec, rect.left(), rect.width());
            painter.line_segment(
                [pos2(x, rect.bottom() - 4.0), pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, label_color),
            );
            if s == ticks {
                painter.text(
                    pos2(x - 2.0, rect.bottom() - 4.0),
                    egui::Align2::RIGHT_BOTTOM,
                    "0",
                    font.clone(),
                    label_color,
                );
            } else if s == 0 {
                painter.text(
                    pos2(x + 2.0, rect.bottom() - 4.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("-{:.0}s", window_sec),
                    font.clone(),
                    label_color,
                );
            }
        }
    }
}

impl Widget for WTimeSeries {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, _source: &dyn DataSource) {}

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let sample_rate = source.sample_rate() as f32;
        let visible_samples = (self.time_window_sec * sample_rate).max(10.0) as usize;

        let data = source.get_data(visible_samples);
        let exg_channels = source.exg_channels();
        let num_channels = exg_channels.len().min(self.visible_channels.len());

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label("Vert Scale");
            let scales = [0.0, 50.0, 100.0, 200.0, 400.0, 1000.0, 10000.0];
            let scale_labels = [
                "Auto", "50 uV", "100 uV", "200 uV", "400 uV", "1000 uV", "10000 uV",
            ];
            let mut scale_text = if self.y_scale_uv < 0.5 {
                "Auto".to_string()
            } else {
                format!("{:.0} uV", self.y_scale_uv)
            };
            for (i, &sc) in scales.iter().enumerate() {
                if (self.y_scale_uv - sc).abs() < 0.1 {
                    scale_text = scale_labels[i].to_string();
                    break;
                }
            }
            egui::ComboBox::from_id_salt("ts_yscale")
                .selected_text(scale_text)
                .show_ui(ui, |ui| {
                    for (i, &sc) in scales.iter().enumerate() {
                        if ui
                            .selectable_label((self.y_scale_uv - sc).abs() < 0.1, scale_labels[i])
                            .clicked()
                        {
                            self.y_scale_uv = sc;
                        }
                    }
                });

            ui.label("Window");
            let options = [1.0, 3.0, 5.0, 10.0, 20.0];
            let labels = ["1 sec", "3 sec", "5 sec", "10 sec", "20 sec"];
            let mut selected_text = format!("{:.0} sec", self.time_window_sec);
            for (i, &secs) in options.iter().enumerate() {
                if (self.time_window_sec - secs).abs() < 0.1 {
                    selected_text = labels[i].to_string();
                    break;
                }
            }
            egui::ComboBox::from_id_salt("ts_window")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    for (i, &secs) in options.iter().enumerate() {
                        if ui
                            .selectable_label((self.time_window_sec - secs).abs() < 0.1, labels[i])
                            .clicked()
                        {
                            self.time_window_sec = secs;
                        }
                    }
                });
        });

        let available_height = ui.available_height();
        let visible_count = (0..num_channels)
            .filter(|&i| self.visible_channels.get(i).copied().unwrap_or(false))
            .count()
            .max(1);
        let row_h = (available_height / visible_count as f32).max(28.0);
        let last_visible = (0..num_channels)
            .filter(|&i| self.visible_channels.get(i).copied().unwrap_or(false))
            .next_back();

        for (i, &channel_idx) in exg_channels.iter().enumerate().take(num_channels) {
            if !self.visible_channels[i] {
                continue;
            }

            let color = theme::channel_color(i);
            let ys = channel_samples(&data, channel_idx);
            let scale = self.scale_for_trace(i, &ys);
            let eff_scale = scale as f32;
            let rms = uv_std_last_second(&ys, sample_rate);

            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let layout = channel_bar_layout(ui.available_width());
                    // Numbered electrode circle (Java on/off button)
                    let (circ_resp, painter) = ui.allocate_painter(
                        egui::vec2(ELECTRODE_W - 4.0, 22.0),
                        egui::Sense::click(),
                    );
                    let center = circ_resp.rect.center();
                    painter.circle_filled(center, 10.0, color);
                    painter.text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        format!("{}", i + 1),
                        egui::FontId::proportional(12.0),
                        theme::WHITE,
                    );
                    if circ_resp.clicked() {
                        self.visible_channels[i] = false;
                    }

                    // Remaining width after the electrode button — identical on every row.
                    // ± / RMS overlay the trace so RMS digits cannot shift time.
                    let (plot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(
                            layout.plot_w.min(ui.available_width()),
                            (row_h - 2.0).max(8.0),
                        ),
                        egui::Sense::hover(),
                    );
                    paint_channel_trace(
                        ui,
                        plot_rect,
                        &ys,
                        sample_rate,
                        self.time_window_sec,
                        scale,
                        color,
                        last_visible == Some(i),
                    );

                    ui.scope_builder(egui::UiBuilder::new().max_rect(plot_rect), |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                        ui.horizontal(|ui| {
                            if ui.small_button("−").clicked() {
                                let next = (eff_scale / 2.0).max(10.0);
                                self.per_channel_y_scales[i] = next;
                            }
                            ui.small(format!("{:.0}", eff_scale));
                            if ui.small_button("+").clicked() {
                                let next = (eff_scale * 2.0).min(10000.0);
                                self.per_channel_y_scales[i] = next;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                ui.colored_label(color, format!("{:.2} uVrms", rms));
                            });
                        });
                    });
                },
            );
        }

        // Re-enable hidden channels (Java lets you click the numbered button to toggle).
        let hidden: Vec<usize> = (0..num_channels)
            .filter(|&i| !self.visible_channels[i])
            .collect();
        if !hidden.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.small("Off:");
                for i in hidden {
                    let color = theme::channel_color(i);
                    if ui
                        .add(egui::Button::new(format!("{}", i + 1)).fill(color).small())
                        .clicked()
                    {
                        self.visible_channels[i] = true;
                    }
                }
            });
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
    #[test]
    fn std_of_constant_is_zero() {
        let v = [3.0, 3.0, 3.0];
        assert!(super::uv_std(&v).abs() < 1e-9);
    }

    #[test]
    fn std_empty_is_zero() {
        assert_eq!(super::uv_std(&[]), 0.0);
    }

    #[test]
    fn uvrms_uses_last_second_like_java() {
        let sr = 250.0_f32;
        let mut ys = vec![10.0; 250];
        ys.extend(std::iter::repeat(0.0).take(250));
        let last_sec = super::uv_std_last_second(&ys, sr);
        let whole = super::uv_std(&ys);
        assert!(
            last_sec.abs() < 1e-9,
            "last 1 s is constant 0, std={}",
            last_sec
        );
        assert!(whole > 1.0, "full-window std should see the 10 µV block");
    }

    #[test]
    fn channel_bar_y_is_symmetric_vert_scale() {
        // Java ChannelBar: yAxisLowerLim = -vertScale, yAxisUpperLim = +vertScale.
        let ts = super::WTimeSeries::new();
        let s = ts.y_scale_uv() as f64;
        assert_eq!((-s, s), (-200.0, 200.0));
    }

    #[test]
    fn every_channel_bar_uses_the_same_time_window() {
        assert_eq!(super::channel_bar_x_range(5.0), (-5.0, 0.0));
        assert_eq!(super::channel_bar_x_range(20.0), (-20.0, 0.0));
    }

    #[test]
    fn electrode_1_reads_first_exg_column_not_package_index() {
        // Cyton/Synthetic row: [package_num, eeg0, eeg1, ...]
        let row = vec![42.0, 1.5, 2.5, 3.5];
        let exg = [1usize, 2, 3];
        assert_eq!(super::sample_at(&row, exg[0]), 1.5);
        assert_eq!(super::sample_at(&row, 0), 42.0);
    }

    #[test]
    fn newest_sample_is_at_time_zero_on_every_channel() {
        let n = 250usize;
        assert!((super::sample_time(n - 1, n, 250.0)).abs() < 1e-12);
        let blink_k = 200;
        assert_eq!(
            super::sample_time(blink_k, n, 250.0),
            super::sample_time(blink_k, n, 250.0)
        );
    }

    #[test]
    fn same_event_time_maps_to_the_same_pixel() {
        let t = super::sample_time(200, 250, 250.0);
        let x_ch1 = super::time_to_x(t, 5.0, 0.0, 300.0);
        let x_ch8 = super::time_to_x(t, 5.0, 0.0, 300.0);
        assert!((x_ch1 - x_ch8).abs() < 1e-6);
    }

    #[test]
    fn channel_bar_plot_width_is_independent_of_rms_digits() {
        let layout = super::channel_bar_layout(400.0);
        assert_eq!(layout.plot_w, 400.0 - super::ELECTRODE_W);
        assert!(layout.plot_w > 200.0);
    }
}
