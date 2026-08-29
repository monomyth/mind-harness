//! Generic BrainFlow-backed board.
//!
//! This is the Rust equivalent of the Java `BoardBrainFlow` + its subclasses.
//! It can represent Synthetic, Cyton (Serial/WiFi), Ganglion (Native/BLE/WiFi), etc.

use crate::board::impedance::{
    column_window, cyton_impedance_off_cmd, cyton_impedance_on_cmd, ganglion_kohm,
    kohm_from_lead_off_std_uv, population_std,
};
use crate::board::{BoardError, DataSource};
use brainflow::board_shim::BoardShim;
use brainflow::brainflow_input_params::BrainFlowInputParamsBuilder;
use brainflow::{BoardIds, BrainFlowPresets, NoiseTypes};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct BrainFlowBoard {
    board: Option<BoardShim>,
    board_id: BoardIds,
    serial_port: Option<String>,
    device_id: Option<String>, // for BLE / Ganglion
    exg_channels: Vec<usize>,
    accel_channels: Vec<usize>,
    sample_rate: i32,
    is_streaming: bool,
    /// Unfiltered BrainFlow rows (Java `dataProcessingRawBuffer`).
    latest_data: Mutex<Vec<Vec<f64>>>,
    /// Full-window IIR of `latest_data` EXG columns (Java `dataProcessingFilteredBuffer`).
    filtered_data: Mutex<Vec<Vec<f64>>>,

    // Basic global filtering (Option A) + per-channel (Option B)
    filter_settings: crate::filter_settings::FilterSettings,

    /// Phase 7: exact count of samples delivered in the most recent board.update() / shim.get_board_data
    /// (used by app for accurate packet loss % in the WPacketLoss SidePanel visual)
    last_delivered: usize,
    last_lost: usize,
    package_num_channel: Option<usize>,
    index_tracker: Option<crate::stream_stats::SampleIndexTracker>,

    /// Impedance check currently running (UI Start/Stop).
    impedance_active: bool,
    resistance_channels: Vec<usize>,
    impedance_values: Vec<Option<f64>>,
    cyton_imp_scan: Option<CytonImpScan>,
    impedance_error: Option<String>,
}

#[derive(Clone, Copy)]
enum CytonImpScan {
    Measuring {
        channel: usize,
        since: Instant,
    },
    /// Stream held after `off`; wait before `on` so the board can ACK (Java 150 ms).
    OffWait {
        next: usize,
        since: Instant,
        resume_stream: bool,
    },
}

/// Dwell long enough for a 1 s std window after ADS/lead-off settles.
const CYTON_IMP_DWELL: Duration = Duration::from_millis(2000);
const CYTON_IMP_OFF_GAP: Duration = Duration::from_millis(150);
const BRAINFLOW_STREAM_CAP: usize = 45000;

impl BrainFlowBoard {
    pub fn new(board_id: BoardIds, serial_port: Option<String>, device_id: Option<String>) -> Self {
        let sample_rate =
            brainflow::board_shim::get_sampling_rate(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or(250) as i32;

        let exg_channels: Vec<usize> =
            brainflow::board_shim::get_exg_channels(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or_default()
                .into_iter()
                .collect();

        let accel_channels: Vec<usize> =
            brainflow::board_shim::get_accel_channels(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or_default()
                .into_iter()
                .collect();

        let num_exg = exg_channels.len();

        let resistance_channels: Vec<usize> = brainflow::board_shim::get_resistance_channels(
            board_id,
            BrainFlowPresets::DefaultPreset,
        )
        .unwrap_or_default();

        let package_num_channel = brainflow::board_shim::get_package_num_channel(
            board_id,
            BrainFlowPresets::DefaultPreset,
        )
        .ok();

        let index_tracker = match board_id {
            BoardIds::CytonBoard | BoardIds::SyntheticBoard => {
                Some(crate::stream_stats::SampleIndexTracker::wrapping_0_255())
            }
            BoardIds::CytonDaisyBoard => {
                Some(crate::stream_stats::SampleIndexTracker::daisy_even_0_254())
            }
            _ => None,
        };

        Self {
            board: None,
            board_id,
            serial_port,
            device_id,
            exg_channels,
            accel_channels,
            sample_rate,
            is_streaming: false,
            latest_data: Mutex::new(Vec::new()),
            filtered_data: Mutex::new(Vec::new()),

            // Per-channel filter settings (Option B foundation)
            filter_settings: crate::filter_settings::FilterSettings::new(num_exg),

            last_delivered: 0,
            last_lost: 0,
            package_num_channel,
            index_tracker,
            impedance_active: false,
            resistance_channels,
            impedance_values: vec![None; num_exg],
            cyton_imp_scan: None,
            impedance_error: None,
        }
    }

    /// Convenience constructor for Synthetic board.
    /// Keep BrainFlow's real EXG indices (never assume column 0 is EEG — it is often the
    /// sample index). Truncate to the user-requested channel count.
    pub fn synthetic(num_channels: usize) -> Self {
        let mut board = Self::new(BoardIds::SyntheticBoard, None, None);
        let n = num_channels.max(1);
        if board.exg_channels.is_empty() {
            board.exg_channels = (0..n).collect();
        } else {
            board.exg_channels.truncate(n);
        }
        let n = board.exg_channels.len();
        board.filter_settings = crate::filter_settings::FilterSettings::new(n);
        board.impedance_values = vec![None; n];
        board
    }

    /// Convenience for Cyton 8-channel via serial
    pub fn cyton_serial(port: &str) -> Self {
        Self::new(BoardIds::CytonBoard, Some(port.to_string()), None)
    }

    /// Convenience for Cyton + Daisy (16-channel) via serial
    pub fn cyton_serial_daisy(port: &str) -> Self {
        Self::new(BoardIds::CytonDaisyBoard, Some(port.to_string()), None)
    }

    /// Ganglion via BrainFlow native BLE (MAC or advertised name).
    pub fn ganglion_native(device_id: &str) -> Self {
        Self::new(
            BoardIds::GanglionNativeBoard,
            None,
            Some(device_id.to_string()),
        )
    }
}

impl DataSource for BrainFlowBoard {
    fn initialize(&mut self) -> Result<(), BoardError> {
        if self.board.is_some() {
            return Ok(());
        }

        let mut builder = BrainFlowInputParamsBuilder::default();

        if let Some(ref port) = self.serial_port {
            builder = builder.serial_port(port.clone());
        }
        if let Some(ref dev) = self.device_id {
            builder = builder.serial_number(dev.clone());
        }

        let params = builder.build();

        let shim = BoardShim::new(self.board_id, params)
            .map_err(|e| BoardError::BrainFlow(e.to_string()))?;

        shim.prepare_session()
            .map_err(|e| BoardError::BrainFlow(e.to_string()))?;

        self.board = Some(shim);
        self.last_delivered = 0;
        self.clear_buffers();
        Ok(())
    }

    fn uninitialize(&mut self) -> Result<(), BoardError> {
        if self.impedance_active {
            let _ = self.stop_impedance_test();
        }
        if let Some(shim) = self.board.take() {
            if self.is_streaming {
                let _ = shim.stop_stream();
            }
            let _ = shim.release_session();
        }
        self.is_streaming = false;
        self.last_delivered = 0;
        self.last_lost = 0;
        if let Some(t) = self.index_tracker.as_mut() {
            t.reset();
        }
        self.impedance_active = false;
        self.cyton_imp_scan = None;
        self.impedance_error = None;
        self.impedance_values.fill(None);
        self.clear_buffers();
        Ok(())
    }

    fn update(&mut self) {
        self.last_delivered = 0;
        self.last_lost = 0;
        if self.impedance_active && self.is_ads1299() && !self.is_streaming {
            self.tick_cyton_impedance();
            return;
        }
        let arr = match self.board.as_ref() {
            Some(shim) if self.is_streaming => shim
                .get_board_data(None, BrainFlowPresets::DefaultPreset)
                .ok(),
            _ => None,
        };
        let Some(arr) = arr else {
            if self.impedance_active && self.is_ads1299() {
                self.tick_cyton_impedance();
            }
            return;
        };
        let n_chans = arr.nrows();
        let n_samples = arr.ncols();
        if n_samples == 0 {
            if self.impedance_active && self.is_ads1299() {
                self.tick_cyton_impedance();
            }
            return;
        }

        // Convert 2D array to row-major samples. Do **not** IIR here:
        // BrainFlow typically returns ~4–8 columns per 60 fps frame, and a
        // cold Butterworth on that length destroys blinks / slow EEG.
        let mut new_samples = Vec::with_capacity(n_samples);
        for s in 0..n_samples {
            let mut row = Vec::with_capacity(n_chans);
            for c in 0..n_chans {
                row.push(arr[[c, s]]);
            }
            new_samples.push(row);
        }

        let delivered = new_samples.len();
        if let (Some(pkg), Some(tracker)) = (self.package_num_channel, self.index_tracker.as_mut())
        {
            let mut lost = 0u64;
            for row in &new_samples {
                if let Some(&v) = row.get(pkg) {
                    lost += tracker.observe(v as i32);
                }
            }
            self.last_lost = lost as usize;
        }
        if self.impedance_active && self.is_ganglion() {
            self.ingest_ganglion_resistance(&new_samples);
        }
        let max_keep = crate::filter_settings::display_buffer_keep(self.sample_rate as usize);
        let filtered = if let Ok(mut guard) = self.latest_data.lock() {
            crate::filter_settings::append_raw_and_filter(
                &mut guard,
                new_samples,
                max_keep,
                &self.exg_channels,
                self.sample_rate as usize,
                &self.filter_settings,
            )
        } else {
            Vec::new()
        };
        if let Ok(mut fg) = self.filtered_data.lock() {
            *fg = filtered;
        }
        self.last_delivered = delivered;
        if self.impedance_active && self.is_ads1299() {
            self.tick_cyton_impedance();
        }
    }

    fn start_streaming(&mut self) -> Result<(), BoardError> {
        if let Some(ref mut shim) = self.board {
            if !self.is_streaming {
                shim.start_stream(BRAINFLOW_STREAM_CAP, "")
                    .map_err(|e| BoardError::BrainFlow(e.to_string()))?;
                self.is_streaming = true;
                self.last_delivered = 0;
                self.last_lost = 0;
                if let Some(t) = self.index_tracker.as_mut() {
                    t.reset();
                }
                self.clear_buffers();
            }
            Ok(())
        } else {
            Err(BoardError::NotInitialized)
        }
    }

    fn stop_streaming(&mut self) -> Result<(), BoardError> {
        if let Some(ref mut shim) = self.board {
            if self.is_streaming {
                let _ = shim.stop_stream();
                self.is_streaming = false;
            }
        }
        Ok(())
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn total_channel_count(&self) -> usize {
        // Approximate – real implementation should query board descriptor
        self.exg_channels.len() + 4
    }

    fn exg_channels(&self) -> &[usize] {
        &self.exg_channels
    }

    fn accel_channels(&self) -> &[usize] {
        &self.accel_channels
    }

    fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    fn get_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        tail_locked(&self.filtered_data, max_samples)
    }

    fn get_raw_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        tail_locked(&self.latest_data, max_samples)
    }

    fn get_frame_data(&self) -> Vec<Vec<f64>> {
        if let Ok(guard) = self.filtered_data.lock() {
            guard.last().cloned().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    fn name(&self) -> &str {
        match self.board_id {
            BoardIds::SyntheticBoard => "BrainFlow Synthetic",
            BoardIds::CytonBoard => "Cyton (8ch)",
            BoardIds::CytonDaisyBoard => "Cyton + Daisy (16ch)",
            BoardIds::GanglionNativeBoard => "Ganglion (Native BLE)",
            _ => "BrainFlow Board",
        }
    }

    // Phase 7: provide filter settings via the DataSource trait so it works
    // uniformly whether the board is BrainFlowBoard or PlaybackBoard (latter returns None).
    fn get_filter_settings(&self) -> Option<&crate::filter_settings::FilterSettings> {
        Some(&self.filter_settings)
    }

    fn set_notch_filter(&mut self, channel: usize, enabled: bool, noise_type: NoiseTypes) {
        self.filter_settings.set_notch(channel, enabled, noise_type);
    }

    fn set_bandpass_filter(&mut self, channel: usize, enabled: bool, low: f64, high: f64) {
        self.filter_settings
            .set_bandpass(channel, enabled, low, high);
    }

    fn apply_pending_filters(&mut self) {
        self.rebuild_filtered();
    }

    fn recent_samples_delivered(&self) -> usize {
        self.last_delivered
    }

    fn recent_samples_lost(&self) -> usize {
        self.last_lost
    }

    fn supports_impedance(&self) -> bool {
        self.is_ads1299() || self.is_ganglion() || self.board_id == BoardIds::SyntheticBoard
    }

    fn start_impedance_test(&mut self, _channels: &[usize]) -> Result<(), BoardError> {
        let n = self.exg_channels.len();
        self.impedance_values = vec![None; n];
        self.cyton_imp_scan = None;

        if self.board_id == BoardIds::SyntheticBoard {
            self.impedance_active = true;
            return Ok(());
        }

        if self.board.is_none() {
            return Err(BoardError::NotInitialized);
        }

        if self.is_ganglion() {
            self.config_board_str("z")?;
            self.impedance_active = true;
            return Ok(());
        }

        if self.is_ads1299() {
            let cmd = cyton_impedance_on_cmd(0)
                .ok_or_else(|| BoardError::Io("Cyton has no EXG channels for impedance".into()))?;
            self.cyton_config_with_stream_paused(&cmd)?;
            self.impedance_active = true;
            self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                channel: 0,
                since: Instant::now(),
            });
            return Ok(());
        }

        Err(BoardError::Io(
            "impedance is not supported on this board".into(),
        ))
    }

    fn stop_impedance_test(&mut self) -> Result<(), BoardError> {
        if let Some(scan) = self.cyton_imp_scan {
            match scan {
                CytonImpScan::Measuring { channel, .. } => {
                    let cmd = cyton_impedance_off_cmd(channel).ok_or_else(|| {
                        BoardError::Io("Cyton has no EXG channels for impedance".into())
                    })?;
                    self.cyton_config_with_stream_paused(&cmd)?;
                }
                CytonImpScan::OffWait { resume_stream, .. } => {
                    self.resume_brainflow_stream(resume_stream)?;
                }
            }
        }
        if self.is_ganglion() && self.impedance_active {
            self.config_board_str("Z")?;
        }
        self.cyton_imp_scan = None;
        self.impedance_active = false;
        self.impedance_values.fill(None);
        Ok(())
    }

    fn get_impedance(&self) -> Vec<Option<f64>> {
        let n = self.exg_channels.len();
        if !self.impedance_active {
            return vec![None; n];
        }
        // Only Synthetic may invent numbers; the UI labels them simulated.
        if self.board_id == BoardIds::SyntheticBoard {
            return (0..n).map(|i| Some(5.0 + (i as f64) * 0.8)).collect();
        }
        let mut out = vec![None; n];
        for (i, v) in self.impedance_values.iter().take(n).enumerate() {
            out[i] = v.filter(|k| *k > 0.0);
        }
        out
    }

    fn impedance_is_simulated(&self) -> bool {
        self.board_id == BoardIds::SyntheticBoard
    }

    fn impedance_quality_kohm(&self) -> (f64, f64) {
        if self.board_id == BoardIds::SyntheticBoard {
            (5.0, 15.0)
        } else if self.is_ads1299() {
            (750.0, 2500.0)
        } else if self.is_ganglion() {
            (50.0, 150.0)
        } else {
            (5.0, 15.0)
        }
    }

    fn impedance_scan_channel(&self) -> Option<usize> {
        if !self.impedance_active {
            return None;
        }
        match self.cyton_imp_scan {
            Some(CytonImpScan::Measuring { channel, .. }) => Some(channel),
            Some(CytonImpScan::OffWait { next, .. }) => Some(next),
            None => None,
        }
    }

    fn impedance_test_active(&self) -> bool {
        self.impedance_active
    }

    fn take_impedance_error(&mut self) -> Option<String> {
        self.impedance_error.take()
    }
}

fn tail_locked(buf: &Mutex<Vec<Vec<f64>>>, max_samples: usize) -> Vec<Vec<f64>> {
    if let Ok(guard) = buf.lock() {
        let len = guard.len();
        let start = len.saturating_sub(max_samples);
        guard[start..].to_vec()
    } else {
        Vec::new()
    }
}

// Inherent methods for BrainFlowBoard (filter controls + internal use)
// Note: set_* are also provided via DataSource trait override so they work on Box<dyn DataSource>
impl BrainFlowBoard {
    fn is_ads1299(&self) -> bool {
        matches!(
            self.board_id,
            BoardIds::CytonBoard | BoardIds::CytonDaisyBoard
        )
    }

    fn is_ganglion(&self) -> bool {
        matches!(self.board_id, BoardIds::GanglionNativeBoard)
    }

    fn config_board_str(&self, cmd: &str) -> Result<(), BoardError> {
        let shim = self.board.as_ref().ok_or(BoardError::NotInitialized)?;
        shim.config_board(cmd)
            .map_err(|e| BoardError::BrainFlow(e.to_string()))?;
        Ok(())
    }

    /// BrainFlow cannot ACK Cyton `x`/`z` while the serial stream is running.
    fn pause_brainflow_stream(&mut self) -> bool {
        if !self.is_streaming {
            return false;
        }
        if let Some(shim) = self.board.as_ref() {
            let _ = shim.stop_stream();
        }
        self.is_streaming = false;
        true
    }

    fn resume_brainflow_stream(&mut self, was_streaming: bool) -> Result<(), BoardError> {
        if !was_streaming || self.is_streaming {
            return Ok(());
        }
        let shim = self.board.as_ref().ok_or(BoardError::NotInitialized)?;
        shim.start_stream(BRAINFLOW_STREAM_CAP, "")
            .map_err(|e| BoardError::BrainFlow(e.to_string()))?;
        self.is_streaming = true;
        self.last_delivered = 0;
        self.last_lost = 0;
        if let Some(t) = self.index_tracker.as_mut() {
            t.reset();
        }
        self.clear_buffers();
        Ok(())
    }

    fn cyton_config_with_stream_paused(&mut self, cmd: &str) -> Result<(), BoardError> {
        let was = self.pause_brainflow_stream();
        let send = self.config_board_str(cmd);
        let resume = self.resume_brainflow_stream(was);
        match send {
            Err(e) => {
                let _ = resume;
                Err(e)
            }
            Ok(()) => resume,
        }
    }

    fn abort_cyton_impedance(&mut self, err: BoardError, resume_stream: bool) {
        tracing::warn!("Cyton impedance aborted: {err}");
        self.impedance_error = Some(err.to_string());
        self.impedance_active = false;
        self.cyton_imp_scan = None;
        let _ = self.resume_brainflow_stream(resume_stream);
    }

    fn ingest_ganglion_resistance(&mut self, rows: &[Vec<f64>]) {
        let n = self.exg_channels.len();
        if self.impedance_values.len() != n {
            self.impedance_values.resize(n, None);
        }
        for row in rows {
            for (i, &col) in self.resistance_channels.iter().take(n).enumerate() {
                if let Some(&v) = row.get(col) {
                    if let Some(kohm) = ganglion_kohm(v) {
                        self.impedance_values[i] = Some(kohm);
                    }
                }
            }
        }
    }

    fn tick_cyton_impedance(&mut self) {
        let n = self.exg_channels.len();
        if n == 0 {
            return;
        }
        match self.cyton_imp_scan {
            Some(CytonImpScan::Measuring { channel, since })
                if since.elapsed() >= CYTON_IMP_DWELL =>
            {
                if let Some(kohm) = self.cyton_kohm_for_channel(channel) {
                    if channel < self.impedance_values.len() {
                        self.impedance_values[channel] = Some(kohm);
                    }
                }
                let next = (channel + 1) % n;
                let Some(off) = cyton_impedance_off_cmd(channel) else {
                    return;
                };
                let was = self.pause_brainflow_stream();
                if let Err(e) = self.config_board_str(&off) {
                    self.abort_cyton_impedance(e, was);
                    return;
                }
                self.cyton_imp_scan = Some(CytonImpScan::OffWait {
                    next,
                    since: Instant::now(),
                    resume_stream: was,
                });
            }
            Some(CytonImpScan::OffWait {
                next,
                since,
                resume_stream,
            }) if since.elapsed() >= CYTON_IMP_OFF_GAP => {
                let Some(on) = cyton_impedance_on_cmd(next) else {
                    self.abort_cyton_impedance(
                        BoardError::Io("Cyton has no EXG channels for impedance".into()),
                        resume_stream,
                    );
                    return;
                };
                if let Err(e) = self.config_board_str(&on) {
                    self.abort_cyton_impedance(e, resume_stream);
                    return;
                }
                if let Err(e) = self.resume_brainflow_stream(resume_stream) {
                    self.abort_cyton_impedance(e, false);
                    return;
                }
                self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                    channel: next,
                    since: Instant::now(),
                });
            }
            _ => {}
        }
    }

    fn cyton_kohm_for_channel(&self, ch: usize) -> Option<f64> {
        let col = *self.exg_channels.get(ch)?;
        let window = self.sample_rate.max(1) as usize;
        let guard = self.latest_data.lock().ok()?;
        let xs = column_window(&guard, col, window)?;
        let std = population_std(&xs)?;
        Some(kohm_from_lead_off_std_uv(std))
    }

    fn clear_buffers(&mut self) {
        if let Ok(mut g) = self.latest_data.lock() {
            g.clear();
        }
        if let Ok(mut g) = self.filtered_data.lock() {
            g.clear();
        }
    }

    fn rebuild_filtered(&mut self) {
        let filtered = if let Ok(guard) = self.latest_data.lock() {
            crate::filter_settings::rebuild_filtered_display(
                &guard,
                &self.exg_channels,
                self.sample_rate as usize,
                &self.filter_settings,
            )
        } else {
            return;
        };
        if let Ok(mut fg) = self.filtered_data.lock() {
            *fg = filtered;
        }
    }

    /// Phase 7: direct accessors for filter state (Option B). Currently the widgets and app
    /// go through the DataSource trait (which delegates), so these are not called directly.
    /// Retained for future widget or inspector use; allowed to keep build clean.
    #[allow(dead_code)]
    pub fn get_filter_settings(&self) -> &crate::filter_settings::FilterSettings {
        &self.filter_settings
    }

    #[allow(dead_code)]
    pub fn get_filter_settings_mut(&mut self) -> &mut crate::filter_settings::FilterSettings {
        &mut self.filter_settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::DataSource;

    #[test]
    fn synthetic_exg_skips_package_num_column() {
        let b = BrainFlowBoard::synthetic(8);
        assert_eq!(
            b.exg_channels(),
            &[1, 2, 3, 4, 5, 6, 7, 8],
            "electrode 1 must be BrainFlow column 1, not package_num 0"
        );
    }

    #[test]
    fn cyton_exg_skips_package_num_column() {
        let b = BrainFlowBoard::cyton_serial("/dev/null");
        assert_eq!(b.exg_channels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn ganglion_exg_skips_package_num_column() {
        let b = BrainFlowBoard::ganglion_native("test");
        assert_eq!(b.exg_channels(), &[1, 2, 3, 4]);
    }

    #[test]
    fn cyton_impedance_is_live_not_simulated() {
        let mut b = BrainFlowBoard::cyton_serial("/dev/null");
        assert!(b.supports_impedance());
        assert!(!b.impedance_is_simulated());
        assert_eq!(b.impedance_quality_kohm(), (750.0, 2500.0));
        assert_eq!(b.get_impedance(), vec![None; 8]);
        assert!(b.start_impedance_test(&[0]).is_err());
        assert!(!b.impedance_test_active());
        assert!(!b.impedance_is_simulated());
        assert_eq!(b.get_impedance(), vec![None; 8]);
        assert!(b.take_impedance_error().is_none());
    }

    #[test]
    fn synthetic_impedance_is_labelled_simulated() {
        let mut b = BrainFlowBoard::synthetic(8);
        assert!(b.supports_impedance());
        assert!(b.impedance_is_simulated());
        assert_eq!(b.impedance_quality_kohm(), (5.0, 15.0));
        assert_eq!(b.get_impedance(), vec![None; 8]);
        b.start_impedance_test(&[0, 1]).unwrap();
        assert!(b.impedance_test_active());
        let vals = b.get_impedance();
        assert_eq!(vals.len(), 8);
        assert!(vals.iter().all(|v| v.is_some()));
        b.stop_impedance_test().unwrap();
        assert!(!b.impedance_test_active());
        assert!(b.get_impedance().iter().all(|v| v.is_none()));
    }

    #[test]
    fn ganglion_impedance_is_live_not_simulated() {
        let mut b = BrainFlowBoard::ganglion_native("test");
        assert!(b.supports_impedance());
        assert!(!b.impedance_is_simulated());
        assert_eq!(b.impedance_quality_kohm(), (50.0, 150.0));
        assert_eq!(b.get_impedance(), vec![None; 4]);
        assert!(b.start_impedance_test(&[0]).is_err());
        assert!(!b.impedance_test_active());
        assert_eq!(b.get_impedance(), vec![None; 4]);
    }
}
