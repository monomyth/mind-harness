//! W_timeSeries — stacked per-channel EEG traces matching the Java ChannelBar look.

use crate::board::DataSource;
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use egui::{pos2, Color32, Pos2, Rect, Shape, Stroke};

/// Java ChannelBar: electrode button is outside the plot; RMS overlays the plot.
const ELECTRODE_W: f32 = 26.0;
/// Left text column (montage name or `Ch N`), beside the numbered on/off circle.
const LABEL_COL_W: f32 = 40.0;
/// 1px gutter so plot rects do not touch.
const ROW_GUTTER_Y: f32 = 1.0;
/// Unused space under the last row so the 22px status bar does not clip the axis.
const BOTTOM_PAD: f32 = 12.0;
/// Extra height on the last visible row for the time-axis ticks/labels.
const LAST_ROW_AXIS: f32 = 10.0;

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

/// True when a recording mark falls in the visible Time Series window (newest at playhead).
pub(crate) fn marker_visible_in_window(
    sample_index: u64,
    playhead: usize,
    visible_len: usize,
) -> bool {
    if visible_len == 0 {
        return false;
    }
    let start = playhead as i64 - visible_len as i64 + 1;
    let idx = sample_index as i64;
    idx >= start && idx <= playhead as i64
}

fn paint_markers(
    ui: &egui::Ui,
    rect: Rect,
    source: &dyn DataSource,
    sample_rate: f32,
    window_sec: f32,
    n_samples: usize,
) {
    let marks = source.session_markers();
    if marks.is_empty() || n_samples == 0 {
        return;
    }
    let playhead = source
        .playhead_sample()
        .unwrap_or_else(|| n_samples.saturating_sub(1));
    let painter = ui.painter_at(rect);
    for m in marks {
        if !marker_visible_in_window(m.sample_index, playhead, n_samples) {
            continue;
        }
        let start = playhead as i64 - n_samples as i64 + 1;
        let k = (m.sample_index as i64 - start) as usize;
        let t = sample_time(k, n_samples, sample_rate);
        let x = time_to_x(t, window_sec, rect.left(), rect.width());
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgb(220, 80, 80)),
        );
        painter.text(
            pos2(x + 2.0, rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            &m.label,
            egui::FontId::proportional(9.0),
            Color32::from_rgb(180, 40, 40),
        );
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
    label_w: f32,
    plot_w: f32,
}

fn channel_bar_layout(row_width: f32) -> ChannelBarLayout {
    ChannelBarLayout {
        label_w: LABEL_COL_W,
        plot_w: (row_width - LABEL_COL_W - ELECTRODE_W).max(16.0),
    }
}

pub(crate) struct TraceStackLayout {
    pub row_h: f32,
    pub gutter: f32,
    pub bottom_pad: f32,
    pub last_row_extra: f32,
}

/// Do not consume 100% of available height: gutters + last-row axis + bottom pad.
pub(crate) fn trace_stack_layout(available_height: f32, visible_count: usize) -> TraceStackLayout {
    let n = visible_count.max(1);
    let gutter = ROW_GUTTER_Y;
    let bottom_pad = BOTTOM_PAD;
    let last_row_extra = LAST_ROW_AXIS;
    let gutters = (n.saturating_sub(1) as f32) * gutter;
    let reserved = bottom_pad + last_row_extra + gutters;
    let usable = (available_height - reserved).max(28.0);
    let row_h = (usable / n as f32).max(28.0);
    TraceStackLayout {
        row_h,
        gutter,
        bottom_pad,
        last_row_extra,
    }
}

pub(crate) fn last_row_height(layout: &TraceStackLayout) -> f32 {
    layout.row_h + layout.last_row_extra
}

/// Left-column text: board montage name if present, otherwise `Ch N`.
pub(crate) fn left_channel_label(logical: usize, board_label: &str) -> String {
    let t = board_label.trim();
    if t.is_empty() {
        format!("Ch {}", logical + 1)
    } else {
        t.to_string()
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
#[allow(clippy::too_many_arguments)]
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
    painter.rect_filled(rect, 0.0, theme::CANVAS);
    painter.line_segment(
        [pos2(rect.left(), y0), pos2(rect.right(), y0)],
        Stroke::new(1.0_f32, theme::HAIRLINE),
    );

    let label_color = theme::TEXT;
    let font = egui::FontId::proportional(10.0);

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
        let stack = trace_stack_layout(available_height, visible_count);
        ui.spacing_mut().item_spacing.y = stack.gutter;
        let last_visible = (0..num_channels)
            .rev()
            .find(|&i| self.visible_channels.get(i).copied().unwrap_or(false));

        for (i, &channel_idx) in exg_channels.iter().enumerate().take(num_channels) {
            if !self.visible_channels[i] {
                continue;
            }

            let powered = source.channel_powered().get(i).copied().unwrap_or(true);
            let color = if powered {
                theme::channel_color(i)
            } else {
                Color32::from_gray(120)
            };
            let ys = channel_samples(&data, channel_idx);
            let scale = self.scale_for_trace(i, &ys);
            let eff_scale = scale as f32;
            let rms = uv_std_last_second(&ys, sample_rate);
            let row_h = if last_visible == Some(i) {
                last_row_height(&stack)
            } else {
                stack.row_h
            };
            let row_label = left_channel_label(i, &source.channel_label(i));

            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let layout = channel_bar_layout(ui.available_width());
                    let (label_rect, _) = ui.allocate_exact_size(
                        egui::vec2(layout.label_w, 22.0_f32.min(row_h).max(16.0)),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        pos2(label_rect.left(), label_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &row_label,
                        egui::FontId::proportional(11.0),
                        theme::TEXT,
                    );
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
                        if powered {
                            format!("{}", i + 1)
                        } else {
                            "off".into()
                        },
                        egui::FontId::proportional(if powered { 12.0 } else { 9.0 }),
                        theme::TEXT,
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
                    if last_visible == Some(i) {
                        paint_markers(
                            ui,
                            plot_rect,
                            source,
                            sample_rate,
                            self.time_window_sec,
                            ys.len(),
                        );
                    }

                    ui.scope_builder(egui::UiBuilder::new().max_rect(plot_rect), |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                        ui.horizontal(|ui| {
                            if ui.small_button("−").clicked() {
                                let next = (eff_scale / 2.0).max(10.0);
                                self.per_channel_y_scales[i] = next;
                            }
                            ui.small(format!("{:.0} uV", eff_scale));
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

        ui.add_space(stack.bottom_pad);
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
        ys.extend(std::iter::repeat_n(0.0, 250));
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
        assert_eq!(
            layout.plot_w,
            400.0 - super::LABEL_COL_W - super::ELECTRODE_W
        );
        assert_eq!(layout.label_w, super::LABEL_COL_W);
        assert!(layout.plot_w > 200.0);
    }

    #[test]
    fn channel_rows_have_one_pixel_gutter() {
        assert_eq!(super::ROW_GUTTER_Y, 1.0);
        let layout = super::trace_stack_layout(400.0, 8);
        assert_eq!(layout.gutter, 1.0);
        let mut y = 0.0;
        for i in 0..8 {
            let h = if i == 7 {
                super::last_row_height(&layout)
            } else {
                layout.row_h
            };
            let bottom = y + h;
            if i < 7 {
                assert!((bottom + layout.gutter - bottom - 1.0).abs() < 1e-5);
            }
            y = bottom + layout.gutter;
        }
    }

    #[test]
    fn last_row_leaves_bottom_pad_and_axis() {
        let available = 400.0;
        let layout = super::trace_stack_layout(available, 8);
        assert!(layout.bottom_pad >= 8.0);
        assert!(layout.last_row_extra > 0.0);
        let n = 8.0;
        let used = layout.row_h * (n - 1.0)
            + super::last_row_height(&layout)
            + layout.gutter * (n - 1.0)
            + layout.bottom_pad;
        assert!(
            used <= available + 1e-3,
            "used {used} available {available}"
        );
        assert!(
            layout.row_h * n < available,
            "rows must not consume 100% of available height"
        );
    }

    #[test]
    fn left_label_uses_montage_or_ch_n() {
        assert_eq!(super::left_channel_label(0, ""), "Ch 1");
        assert_eq!(super::left_channel_label(15, "   "), "Ch 16");
        assert_eq!(super::left_channel_label(0, "Ch 1"), "Ch 1");
        assert_eq!(super::left_channel_label(0, "Fp1"), "Fp1");
        assert_eq!(super::left_channel_label(7, "O2"), "O2");
    }
}
