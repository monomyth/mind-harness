//! W_Focus — Concentration / Relaxation (Mindfulness/Restfulness) widget with real-time audio feedback.
//!
//! Phase 6 implementation: Uses BrainFlow MLModel (Mindfulness or Restfulness metrics
//! with the built-in DefaultClassifier — zero config, no external files required).
//!
//! - Real ML via `brainflow::ml_model::MlModel` + `data_filter::get_avg_band_powers` for accurate feature vectors.
//! - Graceful fallback to an improved alpha/beta band-power proxy (using our shared FFT) when ML is disabled or fails.
//! - Toggleable real-time audio feedback: a soft sine tone whose pitch rises with focus level (200–900 Hz).
//!   Volume is low and non-intrusive by default. Threshold slider controls when tone becomes prominent.
//! - Manual "Mark Focus Event" button that sends timestamped markers via WidgetContext (flows to networking + BDF annotations).
//! - Model file support for advanced users (ONNX custom classifiers downloaded from BrainFlow).
//!
//! Obtaining models:
//!   * Built-in (DefaultClassifier): Nothing to download — compiled into the BrainFlow MLModule.
//!   * Custom ONNX: See https://github.com/brainflow-dev/brainflow/tree/master/src/ml/train
//!     (generated files like `logreg_mindfulness.onnx`, `svm_mindfulness.onnx`, etc.).
//!     Place them anywhere and point the widget at the .onnx path.
//!
//! The Focus widget is now genuinely useful for neurofeedback-style experiments.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::board::DataSource;
use crate::event_log::LogLevel;
use crate::fft::compute_band_powers;
use crate::widgets::Widget;
use brainflow::{
    brainflow_model_params::BrainFlowModelParamsBuilder, data_filter, ml_model::MlModel,
    BrainFlowClassifiers, BrainFlowMetrics,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use ndarray::Array2;

/// Simple real-time audio feedback for the Focus widget.
/// Runs a cpal output stream in the background that generates a sine tone.
/// Pitch (200–900 Hz) and amplitude are modulated by the latest focus value (read via atomics).
/// Phase accumulator is fully lock-free (AtomicU32 holding f32::to_bits()).
/// A test-tone deadline (AtomicU64 millis) allows a clean, racy-free "Test Tone" that forces high pitch for ~1s.
struct FocusAudio {
    enabled: Arc<AtomicBool>,
    focus_encoded: Arc<AtomicU32>,     // focus * 1000 as u32 (0–1000)
    threshold_encoded: Arc<AtomicU32>, // threshold * 1000
    stream: Option<cpal::Stream>,
    sample_rate: u32,
    phase: Arc<AtomicU32>, // f32 phase [0,1) encoded via to_bits() — lock-free for real-time callback
    test_deadline: Arc<AtomicU64>, // millis since UNIX_EPOCH when current test tone expires (0 = inactive)
}

impl FocusAudio {
    fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            focus_encoded: Arc::new(AtomicU32::new(500)),
            threshold_encoded: Arc::new(AtomicU32::new(650)), // 0.65 default
            stream: None,
            sample_rate: 44100,
            phase: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            test_deadline: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_focus(&self, v: f32) {
        let enc = (v.clamp(0.0, 1.0) * 1000.0) as u32;
        self.focus_encoded.store(enc, Ordering::Relaxed);
    }

    pub fn set_threshold(&self, t: f32) {
        let enc = (t.clamp(0.0, 1.0) * 1000.0) as u32;
        self.threshold_encoded.store(enc, Ordering::Relaxed);
    }

    /// Phase 7: audio control API retained for potential external mute / status queries.
    /// Currently the widget owns the handle and drives enable via its own checkbox.
    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Trigger a short test tone (forces high focus for the audio callback for `duration_ms`).
    /// Lock-free and thread-safe. The tone will be high-pitched for the full duration even if
    /// real focus updates continue on the GUI thread.
    pub fn trigger_test_tone(&self, duration_ms: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.test_deadline
            .store(now + duration_ms, Ordering::Relaxed);
        self.set_focus(0.9);
    }

    /// Start or stop the audio stream according to `on`.
    /// Safe to call from the main (egui) thread.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        if on && self.stream.is_none() {
            if let Err(e) = self.start_stream() {
                tracing::warn!(
                    "[FocusAudio] Failed to start audio stream: {}. Audio feedback disabled.",
                    e
                );
                self.enabled.store(false, Ordering::Relaxed);
            }
        } else if !on && self.stream.is_some() {
            self.stop_stream();
        }
    }

    fn start_stream(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default audio output device".to_string())?;

        let config = device
            .default_output_config()
            .map_err(|e| format!("output config: {}", e))?
            .config();

        let sr = config.sample_rate.0;
        self.sample_rate = sr;

        let enabled = self.enabled.clone();
        let focus = self.focus_encoded.clone();
        let threshold = self.threshold_encoded.clone();
        let phase = self.phase.clone();
        let test_deadline = self.test_deadline.clone();

        let err_fn = |err| tracing::error!("[FocusAudio] stream error: {}", err);

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let is_en = enabled.load(Ordering::Relaxed);
                    let f = focus.load(Ordering::Relaxed) as f32 / 1000.0;
                    let thr = threshold.load(Ordering::Relaxed) as f32 / 1000.0;

                    // Test-tone window check (lock-free, real-time safe).
                    // If active, force high focus for the tone only — gives reliable full-duration test
                    // without threads or races with the GUI update loop.
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let dl = test_deadline.load(Ordering::Relaxed);
                    let effective_f = if dl > 0 && now_ms < dl { 0.9 } else { f };

                    // Map focus to frequency (low focus = low tone, high focus = higher pitch)
                    let base_freq = 220.0;
                    let max_freq = 880.0;
                    let freq = base_freq + (max_freq - base_freq) * effective_f;

                    // Volume: very quiet baseline, rises when above threshold (non-intrusive)
                    let base_vol = 0.03;
                    let peak_vol = 0.12;
                    let vol = if is_en {
                        if effective_f > thr {
                            base_vol
                                + (peak_vol - base_vol)
                                    * ((effective_f - thr) / (1.0 - thr)).min(1.0)
                        } else {
                            base_vol * 0.6 // still audible but soft below threshold
                        }
                    } else {
                        0.0
                    };

                    // Lock-free phase accumulator (relaxed ordering is sufficient for audio oscillator)
                    let mut ph = f32::from_bits(phase.load(Ordering::Relaxed));
                    let phase_inc = freq / sr as f32;

                    for sample in data.iter_mut() {
                        let s = (ph * std::f32::consts::TAU).sin() * vol;
                        *sample = s;
                        ph = (ph + phase_inc) % 1.0;
                    }
                    phase.store(ph.to_bits(), Ordering::Relaxed);
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("build stream: {}", e))?;

        stream.play().map_err(|e| format!("play: {}", e))?;
        self.stream = Some(stream);
        tracing::info!("[FocusAudio] Audio feedback stream started (sr={}Hz)", sr);
        Ok(())
    }

    fn stop_stream(&mut self) {
        if let Some(stream) = self.stream.take() {
            // Dropping the stream stops playback cleanly.
            drop(stream);
            tracing::info!("[FocusAudio] Audio feedback stopped");
        }
    }
}

impl Drop for FocusAudio {
    fn drop(&mut self) {
        self.stop_stream();
    }
}

/// The Focus widget itself.
pub struct WFocus {
    title: String,
    focus_value: f32, // 0.0 to 1.0 (clamped)
    // Sparkline buffer kept for the decoder; the rack no longer paints it.
    #[allow(dead_code)]
    history: Vec<f32>,
    #[allow(dead_code)]
    max_points: usize,

    // --- MLModel state (Phase 6) ---
    use_ml: bool,
    metric: BrainFlowMetrics,
    use_custom_onnx: bool, // false = DefaultClassifier (built-in, recommended), true = custom file
    model_file_path: String,
    ml_model: Option<MlModel>,
    ml_prepared: bool,
    last_ml_status: String, // human readable status / last error

    // --- Audio feedback ---
    audio: FocusAudio,
    audio_enabled: bool,
    audio_threshold: f32, // 0.0–1.0

    // For threshold crossing + local event history
    prev_focus: f32,
    recent_events: Vec<(f64, String)>, // (unix ts, "Focus 78%")
    max_events: usize,
}

impl WFocus {
    pub fn new() -> Self {
        let mut audio = FocusAudio::new();
        // Start with audio feedback off (non-intrusive default)
        audio.set_enabled(false);

        Self {
            title: "Focus".to_string(),
            focus_value: 0.5,
            history: Vec::new(),
            max_points: 180,

            use_ml: false, // start in proxy mode; user enables ML with one click
            metric: BrainFlowMetrics::Mindfulness,
            use_custom_onnx: false,
            model_file_path: String::new(),
            ml_model: None,
            ml_prepared: false,
            last_ml_status: "Proxy (alpha/beta)".to_string(),

            audio,
            audio_enabled: false,
            audio_threshold: 0.65,

            prev_focus: 0.5,
            recent_events: Vec::new(),
            max_events: 6,
        }
    }

    /// Attempt to (re)initialize the ML model based on current settings.
    /// Called when user toggles ML on or clicks "Apply Model Settings".
    fn init_ml_model(&mut self) {
        // Release any previous model first (important when switching metric or classifier)
        if let Some(old) = self.ml_model.take() {
            let _ = old.release();
        }
        self.ml_prepared = false;

        let classifier = if self.use_custom_onnx && !self.model_file_path.trim().is_empty() {
            BrainFlowClassifiers::OnnxClassifier
        } else {
            BrainFlowClassifiers::DefaultClassifier
        };

        let mut builder = BrainFlowModelParamsBuilder::new()
            .metric(self.metric)
            .classifier(classifier);

        if classifier == BrainFlowClassifiers::OnnxClassifier {
            builder = builder.file(&self.model_file_path);
        }

        let params = builder.build();

        match MlModel::new(params) {
            Ok(model) => match model.prepare() {
                Ok(()) => {
                    self.ml_model = Some(model);
                    self.ml_prepared = true;
                    let name = match self.metric {
                        BrainFlowMetrics::Mindfulness => "Mindfulness",
                        BrainFlowMetrics::Restfulness => "Restfulness",
                        _ => "Custom",
                    };
                    let cname = if self.use_custom_onnx {
                        "ONNX"
                    } else {
                        "Default"
                    };
                    self.last_ml_status = format!("ML Ready: {} ({})", name, cname);
                    tracing::info!(
                        "[WFocus] MLModel prepared successfully: {}",
                        self.last_ml_status
                    );
                }
                Err(e) => {
                    self.last_ml_status = format!("Prepare failed: {}", e);
                    tracing::error!("[WFocus] ML prepare error: {}", e);
                }
            },
            Err(e) => {
                self.last_ml_status = format!("Model creation failed: {}", e);
                tracing::error!("[WFocus] MlModel::new error: {}", e);
            }
        }
    }

    /// Release the ML model (called on drop / when disabling).
    fn release_ml(&mut self) {
        if let Some(model) = self.ml_model.take() {
            let _ = model.release();
        }
        self.ml_prepared = false;
        if self.use_ml {
            self.last_ml_status = "ML released".to_string();
        }
    }

    /// Compute a simple but improved proxy metric using proper band powers (alpha/beta focused).
    /// Used when ML is disabled or unavailable.
    fn compute_proxy(&self, source: &dyn DataSource, window_size: usize) -> f32 {
        let data = source.get_data(window_size);
        if data.is_empty() {
            return self.focus_value; // hold last
        }

        let exg = source.exg_channels();
        let num_chans = exg.len();
        if num_chans == 0 {
            return self.focus_value;
        }

        // Average first up to 4 EXG channels using the authoritative indices from DataSource.
        // This matches exactly what the ML path does (get_avg_band_powers with exg_channels())
        // and works even if a board ever reports non-contiguous or non-zero-based EXG indices.
        let use_chans = num_chans.min(4);
        let mut averaged = vec![0.0f64; data.len()];
        for (i, row) in data.iter().enumerate() {
            for &ch_idx in exg.iter().take(use_chans) {
                if let Some(&v) = row.get(ch_idx) {
                    averaged[i] += v;
                }
            }
            if use_chans > 0 {
                averaged[i] /= use_chans as f64;
            }
        }

        let band_powers = compute_band_powers(&averaged, source.sample_rate() as f64);
        // band_powers: [("Delta",..), ("Theta",..), ("Alpha",..), ("Beta",..), ("Gamma",..)]

        let alpha = band_powers.get(2).map(|(_, p)| *p).unwrap_or(0.0);
        let beta = band_powers.get(3).map(|(_, p)| *p).unwrap_or(0.0);

        // Classic proxy: higher alpha relative to beta → more "relaxed/focused" state in many neurofeedback protocols
        let ratio = if beta > 1e-9 {
            (alpha / (alpha + beta)).clamp(0.0, 1.0)
        } else {
            0.5
        };

        // Map to a pleasant 0.25–0.85 range so the meter moves nicely
        (0.25 + ratio * 0.6) as f32
    }

    /// Compact transport chip: scaled ring (percent in the center) + value.
    pub fn paint_transport_chip(&self, ui: &mut egui::Ui) {
        let value = self.focus_value;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal(|ui| {
                ui.set_max_height(24.0);
                paint_focus_ring(ui, value, 22.0);
                ui.label(
                    egui::RichText::new(format!("{:.2}", value))
                        .small()
                        .color(crate::theme::TEXT),
                );
            });
        });
    }
}

impl Drop for WFocus {
    fn drop(&mut self) {
        self.release_ml();
    }
}

impl Default for WFocus {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WFocus {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let window_size = (source.sample_rate() as usize * 2).min(512);

        let new_focus = if self.use_ml && self.ml_prepared {
            // --- Real BrainFlow MLModel path ---
            // Build board-shaped Array2 (channels x samples) and call BrainFlow's exact band-power extractor.
            let data = source.get_data(window_size);
            if data.is_empty() {
                self.focus_value // hold previous
            } else {
                let n_samples = data.len();
                let n_total_chans = if n_samples > 0 { data[0].len() } else { 0 };
                if n_total_chans == 0 {
                    self.focus_value
                } else {
                    let mut board_arr = Array2::<f64>::zeros((n_total_chans, n_samples));
                    for (s, row) in data.iter().enumerate() {
                        for (c, &val) in row.iter().enumerate().take(n_total_chans) {
                            board_arr[[c, s]] = val;
                        }
                    }

                    let eeg_chans: Vec<usize> = source.exg_channels().to_vec();
                    let sr = source.sample_rate() as usize;

                    let feature_res =
                        data_filter::get_avg_band_powers(board_arr, eeg_chans, sr, true);

                    match feature_res {
                        Ok((mut avg_powers, _stddevs)) => {
                            if let Some(ref model) = self.ml_model {
                                // predict consumes &mut feature vector (5 values for the 5 bands)
                                match model.predict(&mut avg_powers) {
                                    Ok(out) if !out.is_empty() => {
                                        out[0] as f32 // the model returns a single probability-like value in [0,1]
                                    }
                                    Ok(_) => self.compute_proxy(source, window_size),
                                    Err(e) => {
                                        self.last_ml_status = format!("predict err: {}", e);
                                        self.compute_proxy(source, window_size)
                                    }
                                }
                            } else {
                                self.compute_proxy(source, window_size)
                            }
                        }
                        Err(e) => {
                            self.last_ml_status = format!("bandpower err: {}", e);
                            self.compute_proxy(source, window_size)
                        }
                    }
                }
            }
        } else {
            // --- Improved proxy (alpha relative power via proper FFT) ---
            self.compute_proxy(source, window_size)
        };

        self.focus_value = new_focus.clamp(0.0, 1.0);

        // Update audio driver (lock-free)
        self.audio.set_focus(self.focus_value);
        self.audio.set_threshold(self.audio_threshold);

        // History for sparkline
        self.history.push(self.focus_value);
        if self.history.len() > self.max_points {
            self.history.remove(0);
        }

        // Simple threshold-crossing detection (for future auto-features or logging)
        if self.audio_enabled
            && self.focus_value > self.audio_threshold
            && self.prev_focus <= self.audio_threshold
        {
            tracing::info!(
                "[WFocus] Focus crossed audio threshold: {:.2}",
                self.focus_value
            );
        }
        self.prev_focus = self.focus_value;
    }

    /// ML / audio / threshold only. The meter lives on the transport, not here.
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        _source: &dyn DataSource,
        ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.small(if self.use_ml && self.ml_prepared {
            &self.last_ml_status
        } else {
            "Proxy mode"
        });

        // === ML Controls ===
        ui.group(|ui| {
            ui.horizontal(|ui| {
                let mut use_ml = self.use_ml;
                if ui.checkbox(&mut use_ml, "Use BrainFlow ML Model").changed() {
                    self.use_ml = use_ml;
                    if self.use_ml && !self.ml_prepared {
                        self.init_ml_model();
                        if !self.ml_prepared {
                            // Prepare failed — auto-uncheck so header + checkbox are consistent
                            // (error message remains visible in the ML controls group below).
                            self.use_ml = false;
                        }
                    } else if !self.use_ml {
                        self.release_ml();
                        self.last_ml_status = "Proxy (alpha/beta)".to_string();
                    }
                    ctx.log_event(
                        if self.use_ml {
                            LogLevel::Info
                        } else {
                            LogLevel::Warn
                        },
                        "Focus",
                        &format!(
                            "ML model {}",
                            if self.use_ml {
                                "enabled"
                            } else {
                                "disabled (proxy mode)"
                            }
                        ),
                    );
                }
                ui.separator();
                ui.label("Metric:");
                egui::ComboBox::from_id_salt("focus_metric")
                    .selected_text(match self.metric {
                        BrainFlowMetrics::Mindfulness => "Mindfulness",
                        BrainFlowMetrics::Restfulness => "Restfulness",
                        _ => "Custom",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.metric,
                            BrainFlowMetrics::Mindfulness,
                            "Mindfulness",
                        );
                        ui.selectable_value(
                            &mut self.metric,
                            BrainFlowMetrics::Restfulness,
                            "Restfulness",
                        );
                    });
            });

            ui.horizontal(|ui| {
                let mut custom = self.use_custom_onnx;
                if ui.checkbox(&mut custom, "Custom ONNX classifier").changed() {
                    self.use_custom_onnx = custom;
                    if self.use_ml {
                        // user changed type — re-init when they click apply
                    }
                }
                ui.add_enabled_ui(self.use_custom_onnx, |ui| {
                    ui.label("File:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.model_file_path).desired_width(220.0),
                    );
                });
            });

            ui.horizontal(|ui| {
                if ui.button("Apply / Reload Model").clicked() && self.use_ml {
                    self.init_ml_model();
                    if !self.ml_prepared {
                        // Same UX fix as checkbox: on failure do not leave the widget in a
                        // "ML requested but actually proxy" inconsistent state.
                        self.use_ml = false;
                    }
                }
                if ui.button("Release ML").clicked() {
                    self.release_ml();
                    self.last_ml_status = "ML released (proxy active)".to_string();
                }
                ui.small("Default = built-in (no file). ONNX = user-provided .onnx");
            });

            if !self.last_ml_status.is_empty() {
                let is_error = self.last_ml_status.contains("fail")
                    || self.last_ml_status.contains("error")
                    || self.last_ml_status.contains("Prepare");
                let status_color = if is_error {
                    egui::Color32::from_rgb(200, 80, 80)
                } else {
                    egui::Color32::from_gray(120)
                };
                ui.small(egui::RichText::new(&self.last_ml_status).color(status_color));
            }
        });

        let pct = (self.focus_value * 100.0).clamp(0.0, 100.0);

        // === Audio Feedback (Phase 6 highlight) ===
        ui.group(|ui| {
            ui.label(egui::RichText::new("Audio Feedback").strong());

            let mut audio_on = self.audio_enabled;
            if ui
                .checkbox(&mut audio_on, "Enable focus tone (pitch rises with focus)")
                .changed()
            {
                self.audio_enabled = audio_on;
                self.audio.set_enabled(audio_on);
            }

            ui.horizontal(|ui| {
                ui.label("Threshold:");
                if ui
                    .add(egui::Slider::new(&mut self.audio_threshold, 0.40..=0.90).step_by(0.01))
                    .changed()
                {
                    self.audio.set_threshold(self.audio_threshold);
                }
                ui.small(format!("{:.0}%", self.audio_threshold * 100.0));
            });

            ui.horizontal(|ui| {
                if ui.button("Test Tone (1s)").clicked() {
                    // Clean, lock-free test: audio callback will force high pitch for the full 900 ms
                    // using its own deadline (even while GUI continues to push real focus values).
                    // Visual bar gets an immediate pop; real focus resumes automatically after the window.
                    self.audio.trigger_test_tone(900);
                    self.focus_value = 0.9;
                    ctx.log_event(
                        LogLevel::Info,
                        "Focus",
                        "Test tone triggered (1s high pitch)",
                    );
                }
                ui.small("Soft sine tone. Safe for long sessions at low volume.");
            });
        });

        ui.add_space(6.0);

        // === WidgetContext integration: manual focus marker ===
        ui.horizontal(|ui| {
            if ui.button("Mark Current Focus Event").clicked() {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64();
                let text = format!("Focus {:.0}%", pct);
                ctx.send_marker(ts, &text);
                ctx.log_event(
                    LogLevel::Marker,
                    "Focus",
                    &format!("Manual focus marker sent at {:.0}%", pct),
                );

                self.recent_events.push((ts, text));
                if self.recent_events.len() > self.max_events {
                    self.recent_events.remove(0);
                }
            }
            if ui.button("Clear Events").clicked() {
                self.recent_events.clear();
            }
        });

        if !self.recent_events.is_empty() {
            ui.small("Recent focus events:");
            for (ts, txt) in self.recent_events.iter().rev().take(4) {
                ui.small(format!("{:.1}s — {}", ts, txt));
            }
        } else {
            ui.small("Tip: Use 'Mark Current Focus Event' to record high-focus moments into your BDF/CSV + network streams.");
        }

        ui.add_space(4.0);
        ui.small("Tip: Enable ML + audio, watch the tone pitch rise as you focus. Great for closed-loop neurofeedback.");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn paint_focus_ring(ui: &mut egui::Ui, value: f32, diameter: f32) {
    let d = diameter.max(12.0);
    let size = egui::vec2(d, d);
    let (resp, painter) = ui.allocate_painter(size, egui::Sense::hover());
    let c = resp.rect.center();
    let stroke = if d < 36.0 { 2.0_f32 } else { 3.5_f32 };
    let r = (d * 0.5 - stroke).max(4.0);
    painter.circle_stroke(c, r, egui::Stroke::new(stroke, crate::theme::HAIRLINE));
    let steps = if d < 36.0 { 36usize } else { 72usize };
    let filled = ((value.clamp(0.0, 1.0) * steps as f32).round() as usize).min(steps);
    if filled >= 1 {
        let mut pts = Vec::with_capacity(filled + 1);
        for i in 0..=filled {
            let t = i as f32 / steps as f32;
            let a = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::TAU;
            pts.push(egui::pos2(c.x + r * a.cos(), c.y + r * a.sin()));
        }
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(
                pts,
                egui::Stroke::new(stroke, crate::theme::ACCENT),
            ));
        }
    }
    let font = (d * 0.32).clamp(7.0, 16.0);
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0),
        egui::FontId::proportional(font),
        crate::theme::TEXT,
    );
}
