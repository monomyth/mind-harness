//! Generic BrainFlow-backed board.
//!
//! This is the Rust equivalent of the Java `BoardBrainFlow` + its subclasses.
//! It can represent Synthetic, Cyton (Serial/WiFi), Ganglion (Native/BLE/WiFi), etc.

use crate::board::{BoardError, DataSource};
use brainflow::board_shim::BoardShim;
use brainflow::brainflow_input_params::BrainFlowInputParamsBuilder;
use brainflow::{BoardIds, BrainFlowPresets, NoiseTypes};
use std::sync::Mutex;

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

    /// Impedance check currently running (UI Start/Stop). Hardware path is best-effort;
    /// Synthetic returns clearly-labelled simulated values only while this is true.
    impedance_active: bool,
}

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
        board.filter_settings = crate::filter_settings::FilterSettings::new(board.exg_channels.len());
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
        self.clear_buffers();
        Ok(())
    }

    fn update(&mut self) {
        self.last_delivered = 0;
        self.last_lost = 0;
        if let Some(ref mut shim) = self.board {
            if self.is_streaming {
                if let Ok(arr) = shim.get_board_data(None, BrainFlowPresets::DefaultPreset) {
                    let n_chans = arr.nrows();
                    let n_samples = arr.ncols();
                    if n_samples == 0 {
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
                    if let (Some(pkg), Some(tracker)) =
                        (self.package_num_channel, self.index_tracker.as_mut())
                    {
                        let mut lost = 0u64;
                        for row in &new_samples {
                            if let Some(&v) = row.get(pkg) {
                                lost += tracker.observe(v as i32);
                            }
                        }
                        self.last_lost = lost as usize;
                    }
                    let max_keep =
                        crate::filter_settings::display_buffer_keep(self.sample_rate as usize);
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
                }
            }
        }
    }

    fn start_streaming(&mut self) -> Result<(), BoardError> {
        if let Some(ref mut shim) = self.board {
            if !self.is_streaming {
                shim.start_stream(45000, "")
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
        matches!(
            self.board_id,
            BoardIds::CytonBoard | BoardIds::CytonDaisyBoard | BoardIds::GanglionNativeBoard
        )
    }

    fn start_impedance_test(&mut self, _channels: &[usize]) -> Result<(), BoardError> {
        self.impedance_active = true;
        if let Some(ref mut shim) = self.board {
            // Best-effort BrainFlow command. Real kΩ values are only synthesized for
            // SyntheticBoard; Cyton/Ganglion return None until a confirmed API exists.
            let _ = shim.config_board("startimp");
        }
        Ok(())
    }

    fn stop_impedance_test(&mut self) -> Result<(), BoardError> {
        self.impedance_active = false;
        if let Some(ref mut shim) = self.board {
            let _ = shim.config_board("stopimp");
        }
        Ok(())
    }

    fn get_impedance(&self) -> Vec<Option<f64>> {
        let n = self.exg_channels.len();
        if !self.impedance_active {
            return vec![None; n];
        }
        // Only Synthetic is allowed to invent numbers, and the UI labels them as simulated.
        if self.board_id == BoardIds::SyntheticBoard {
            return (0..n)
                .map(|i| Some(5.0 + (i as f64) * 0.8))
                .collect();
        }
        vec![None; n]
    }

    fn impedance_is_simulated(&self) -> bool {
        self.board_id == BoardIds::SyntheticBoard
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
}
