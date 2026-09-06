//! W_fft — Frequency domain plot widget.
//!
//! Port of the original W_fft.pde.

use crate::board::DataSource;
use crate::fft::{fft_display_uv, unmatched_mains_hz};
use crate::filter_settings::NotchMode;
use crate::laterality::latch_rails;
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use egui_plot::{CoordinatesFormatter, Corner, Line, LineStyle, Plot, PlotPoints, VLine};

#[allow(clippy::upper_case_acronyms)]
/// Java `xLimOptions` in W_FFT.pde.
const MAX_FREQ_OPTIONS: &[f64] = &[20.0, 40.0, 60.0, 100.0, 120.0, 250.0, 500.0, 800.0];
/// Java `yLimOptions` / Max uV dropdown.
const MAX_UV_OPTIONS: &[f64] = &[10.0, 50.0, 100.0, 1000.0];
/// Java `fft_plot.setYLim(0.1, yLim)`.
const LOG_YMIN: f64 = -1.0; // log10(0.1)
/// Java DataProcessing FFT smooth floor.
const SMOOTH_MIN_UV: f64 = 0.01;

#[allow(clippy::upper_case_acronyms)]
pub struct WFFT {
    title: String,
    max_freq: f64,
    max_uv: f64,
    smoothing_index: usize, // index into SMOOTH_FACTORS (0.0 = raw, higher = more temporal averaging)
    window_sec: f32,
    // Previous smoothed magnitudes per channel (for exponential smoothing over frames)
    prev_mags: Vec<Vec<f64>>,
    railed: [bool; 8],
}

impl WFFT {
    pub fn new() -> Self {
        Self {
            title: "FFT Plot".to_string(),
            max_freq: 100.0,    // past 60 Hz so a 60 Hz notch is an interior dip, not an axis-edge spike
            max_uv: 100.0,      // Java `yLimOptions[2]`
            smoothing_index: 2, // default 0.75 — matches original Java GUI
            window_sec: 5.0,
            prev_mags: vec![],
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

impl Default for WFFT {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WFFT {
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
        let sample_rate = source.sample_rate() as f64;
        let nyquist = (sample_rate / 2.0).max(1.0);
        let display_max = self.max_freq.min(nyquist);

        ui.horizontal(|ui| {
            ui.label("Max Freq");
            egui::ComboBox::from_id_salt("fft_max_freq")
                .selected_text(format!("{:.0} Hz", self.max_freq))
                .show_ui(ui, |ui| {
                    for &hz in MAX_FREQ_OPTIONS {
                        let label = if hz > nyquist {
                            format!("{:.0} Hz (>{:.0} Nyquist)", hz, nyquist)
                        } else {
                            format!("{:.0} Hz", hz)
                        };
                        if ui
                            .selectable_label((self.max_freq - hz).abs() < 0.1, label)
                            .clicked()
                        {
                            self.max_freq = hz;
                        }
                    }
                });

            ui.label("Max uV");
            egui::ComboBox::from_id_salt("fft_max_uv")
                .selected_text(format!("{:.0} uV", self.max_uv))
                .show_ui(ui, |ui| {
                    for &uv in MAX_UV_OPTIONS {
                        if ui
                            .selectable_label(
                                (self.max_uv - uv).abs() < 0.1,
                                format!("{:.0} uV", uv),
                            )
                            .clicked()
                        {
                            self.max_uv = uv;
                        }
                    }
                });

        });

        // Java nfft at 250 Hz is 256 (~1 Hz/bin), so a 1 Hz high-pass is invisible because the first bin is the corner.
        let nfft = crate::fft::nfft_safe(source.sample_rate()).max(1024);
        let want = ((self.window_sec as f64) * sample_rate).round() as usize;
        let data = source.get_data(want.max(nfft));
        let exg = source.exg_channels();
        let raw_rows = source.get_raw_data(nfft.max(32));
        let mut raw_chs: Vec<Vec<f64>> = Vec::new();
        for &col in exg.iter().take(8) {
            raw_chs.push(
                raw_rows
                    .iter()
                    .map(|row| row.get(col).copied().unwrap_or(0.0))
                    .collect(),
            );
        }
        latch_rails(&mut self.railed, &raw_chs);
        let notch = source
            .get_filter_settings()
            .and_then(|s| s.channels.first())
            .map(NotchMode::from_channel)
            .unwrap_or(NotchMode::Off);

        let smoothing_factor = crate::widgets::SMOOTH_FACTORS
            .get(self.smoothing_index)
            .copied()
            .unwrap_or(0.75) as f64;

        let mut envelopes: Option<(Vec<f64>, Vec<f64>)> = None;
        let mut traces: Vec<(usize, Vec<f64>, Vec<f64>)> = Vec::new();
        for (i, &ch) in exg.iter().enumerate() {
            if i < 8 && self.railed[i] {
                continue;
            }
            let ch_data: Vec<f64> = data
                .iter()
                .map(|row| row.get(ch).copied().unwrap_or(0.0))
                .collect();
            let (freqs, mut mags) = fft_display_uv(&ch_data, sample_rate, display_max);
            if self.prev_mags.len() <= i {
                self.prev_mags.resize(i + 1, vec![]);
            }
            let prev = &mut self.prev_mags[i];
            if prev.len() != mags.len() {
                *prev = mags.clone();
            } else {
                // Java DataProcessing: smooth in log-power space, then back to µV.
                for (p, m) in prev.iter_mut().zip(mags.iter_mut()) {
                    let a = m.max(SMOOTH_MIN_UV).ln();
                    let b = p.max(SMOOTH_MIN_UV).ln();
                    *m = ((1.0 - smoothing_factor) * a + smoothing_factor * b).exp();
                    *p = *m;
                }
            }
            if let Some((_, env)) = envelopes.as_mut() {
                for (e, m) in env.iter_mut().zip(mags.iter()) {
                    *e = e.max(*m);
                }
            } else {
                envelopes = Some((freqs.clone(), mags.clone()));
            }
            traces.push((i, freqs, mags));
        }

        if let Some((freqs, mags)) = envelopes.as_ref() {
            if let Some(hz) = unmatched_mains_hz(freqs, mags, notch) {
                ui.colored_label(
                    theme::ACCENT,
                    format!(
                        "Sharp {hz:.0} Hz peak — Notch {} does not remove it. Try 50 + 60 Hz.",
                        notch.label()
                    ),
                );
            }
        }

        let plot_height = (ui.available_height() - 16.0).max(100.0);
        let log_ymax = self.max_uv.max(1.0).log10();

        let plot = crate::widgets::lock_plot_interaction(Plot::new("fft"))
            .height(plot_height)
            .show_axes([true, true])
            .show_grid(true)
            .auto_bounds([false, false])
            .include_x(crate::fft::FFT_DISPLAY_MIN_HZ)
            .include_x(display_max)
            .include_y(LOG_YMIN)
            .include_y(log_ymax)
            .default_x_bounds(crate::fft::FFT_DISPLAY_MIN_HZ, display_max)
            .default_y_bounds(LOG_YMIN, log_ymax)
            .y_axis_min_width(52.0)
            .x_axis_label("Frequency (Hz)")
            .y_axis_label("Amplitude (uV)")
            .x_axis_formatter(|mark, _| {
                if (mark.value - 0.1).abs() < 0.02 {
                    "0.1".into()
                } else {
                    crate::widgets::axis_tick_label(mark.value)
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
            .coordinates_formatter(
                Corner::LeftTop,
                CoordinatesFormatter::new(|pt, _| {
                    format!("{:.1} Hz\n{:.2} uV", pt.x, 10f64.powf(pt.y))
                }),
            );

        plot.show(ui, |plot_ui| {
            for (i, freqs, mags) in traces {
                let points: PlotPoints = freqs
                    .into_iter()
                    .zip(mags)
                    .map(|(f, uv)| [f, uv.max(0.1).log10()])
                    .collect();
                let color = theme::channel_color(i);
                plot_ui.line(
                    Line::new(
                        crate::widgets::head_plot::LABELS
                            .get(i)
                            .copied()
                            .unwrap_or("?"),
                        points,
                    )
                        .color(color)
                        .width(1.3_f32),
                );
            }
            let mains = egui::Color32::from_white_alpha(40);
            if display_max >= 50.0 {
                plot_ui.vline(
                    VLine::new("50 Hz", 50.0)
                        .color(mains)
                        .width(1.0_f32)
                        .style(LineStyle::dashed_dense()),
                );
            }
            if display_max >= 60.0 {
                plot_ui.vline(
                    VLine::new("60 Hz", 60.0)
                        .color(mains)
                        .width(1.0_f32)
                        .style(LineStyle::dashed_dense()),
                );
            }
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
