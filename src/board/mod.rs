//! Board / DataSource abstraction for the OpenBCI GUI Rust port.
//!
//! This module mirrors the Java `DataSource` interface + `Board` abstract class
//! from the original Processing implementation.
//!
//! The goal is to have a uniform API whether the source is:
//! - BrainFlowSynthetic
//! - Cyton (Serial / WiFi)
//! - Ganglion (Native BLE / BLED112 / WiFi)
//! - Playback file
//! - LSL stream in (future)

pub mod ads_settings;
pub mod ble_scan;
pub mod brainflow_board;
pub mod impedance;
pub mod playback;
pub mod sd_card;
pub mod synthetic;

/// The core contract that every data source (real board or synthetic) must implement.
/// Directly translated from DataSource.pde in the Java GUI.
pub trait DataSource: Send + Sync {
    /// One-time initialization (open session, prepare BrainFlow, etc.)
    fn initialize(&mut self) -> Result<(), BoardError>;

    /// Clean shutdown
    fn uninitialize(&mut self) -> Result<(), BoardError>;

    /// Called every frame / update tick to pull new samples into internal buffers.
    fn update(&mut self);

    /// Start the data stream (BrainFlow start_stream)
    fn start_streaming(&mut self) -> Result<(), BoardError>;

    /// Stop the data stream
    fn stop_streaming(&mut self) -> Result<(), BoardError>;

    /// Is the board currently streaming?
    fn is_streaming(&self) -> bool;

    /// Total number of channels (EXG + accel + other + timestamp + sample index + marker, etc.)
    /// Phase 7: part of the DataSource contract (implemented by all boards). Not yet called from
    /// the UI (widgets use the specialized exg/accel accessors), but kept for parity / future.
    #[allow(dead_code)]
    fn total_channel_count(&self) -> usize;

    /// Which indices are the actual EXG (EEG) channels.
    fn exg_channels(&self) -> &[usize];

    /// Which indices are the accelerometer channels (usually 3 axes).
    fn accel_channels(&self) -> &[usize];

    /// Sample rate in Hz
    fn sample_rate(&self) -> i32;

    /// Get up to `max_samples` of the most recent **filtered** data (widgets / networking).
    /// Each inner Vec<f64> is one "row" (all channels for one sample).
    fn get_data(&self, max_samples: usize) -> Vec<Vec<f64>>;

    /// Unfiltered board rows (Java `dataProcessingRawBuffer`). Used for ODF/BDF recording.
    fn get_raw_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        self.get_data(max_samples)
    }

    /// Get the data from the most recent frame only (what BrainFlow just delivered).
    #[allow(dead_code)]
    fn get_frame_data(&self) -> Vec<Vec<f64>>;

    /// Human readable name of this board (for UI / logging)
    fn name(&self) -> &str;

    // Phase 7 Playback controls (exposed via trait so app status bar can drive without knowing concrete type)
    fn playback_progress(&self) -> Option<(usize, usize)> {
        None
    }
    fn set_playback_speed(&mut self, _speed: f32) {}
    fn playback_speed(&self) -> Option<f32> {
        None
    }
    /// Phase 7 Playback polish: pause/resume the time cursor (no-op for live boards)
    fn toggle_playback_pause(&mut self) {}
    /// Phase 7 Playback polish: scrub to 0.0–1.0 fraction of the recording (no-op for live)
    fn seek_to_fraction(&mut self, _frac: f32) {}

    /// Phase 7 WPacketLoss visual accuracy (plan.md Phase 7 polish):
    /// Return the number of new samples delivered by the most recent call to update().
    /// The app uses this (summed over the measurement window) for a correct expected-vs-received
    /// packet loss % instead of the previous buggy "get_data(1).len()" which was always ~1.
    /// Playback naturally reports the exact advance count so roundtrips show 0% loss.
    fn recent_samples_delivered(&self) -> usize {
        0
    }

    /// Samples missing between indices in the most recent `update()` (Java PacketLossTracker).
    fn recent_samples_lost(&self) -> usize {
        0
    }

    // === Filtering controls (Option B foundation) ===
    fn set_notch_filter(
        &mut self,
        _channel: usize,
        _enabled: bool,
        _noise_type: brainflow::NoiseTypes,
    ) {
    }
    fn set_bandpass_filter(&mut self, _channel: usize, _enabled: bool, _low: f64, _high: f64) {}
    fn get_filter_settings(&self) -> Option<&crate::filter_settings::FilterSettings> {
        None
    }
    /// Flush deferred filter work (Playback rebuilds the whole file once).
    fn apply_pending_filters(&mut self) {}

    // === Impedance test support (Cyton / Ganglion hardware diagnostics) ===
    /// Whether this board type can perform real impedance measurements.
    fn supports_impedance(&self) -> bool {
        false
    }

    /// Begin impedance check on the given EXG channel indices (0-based within exg_channels()).
    /// For real boards this typically switches the hardware into a test current-injection mode.
    fn start_impedance_test(&mut self, _channels: &[usize]) -> Result<(), BoardError> {
        Ok(())
    }

    /// Stop any ongoing impedance test and return hardware to normal streaming.
    fn stop_impedance_test(&mut self) -> Result<(), BoardError> {
        Ok(())
    }

    /// Return the latest impedance reading (kΩ) for each channel, or None if not measured / unsupported.
    /// Length should match exg_channels().len().
    fn get_impedance(&self) -> Vec<Option<f64>> {
        vec![]
    }

    /// True when impedance values are a simulation (Synthetic / Playback), never live hardware.
    fn impedance_is_simulated(&self) -> bool {
        false
    }

    /// `(green_max_kΩ, yellow_max_kΩ)` for contact coloring. Live Cyton uses Java 750 / 2500.
    fn impedance_quality_kohm(&self) -> (f64, f64) {
        (5.0, 15.0)
    }

    /// 0-based EXG index currently injecting lead-off (Cyton scan), if any.
    fn impedance_scan_channel(&self) -> Option<usize> {
        None
    }

    /// True while Start Impedance has succeeded and Stop has not (drives the Testing LED).
    fn impedance_test_active(&self) -> bool {
        false
    }

    /// Drain a mid-scan `config_board` failure so the app can log it.
    fn take_impedance_error(&mut self) -> Option<String> {
        None
    }

    /// ADS1299 per-channel settings (Cyton / Synthetic). None on Ganglion / Playback.
    fn ads_channels(&self) -> Option<&[ads_settings::AdsChannel]> {
        None
    }

    fn commit_ads_channel(
        &mut self,
        _channel: usize,
        _settings: ads_settings::AdsChannel,
    ) -> Result<(), BoardError> {
        Err(BoardError::Io("hardware settings not supported".into()))
    }

    fn channel_powered(&self) -> Vec<bool> {
        vec![true; self.exg_channels().len()]
    }

    fn analog_channels(&self) -> &[usize] {
        &[]
    }

    fn digital_channels(&self) -> &[usize] {
        &[]
    }

    /// Cyton `/0` default, `/2` analog, `/3` digital.
    fn cyton_board_mode(&self) -> Option<u8> {
        None
    }

    fn set_cyton_board_mode(&mut self, _mode: u8) -> Result<(), BoardError> {
        Err(BoardError::Io("board mode not supported".into()))
    }

    /// Analog / Digital / Pulse widgets. False on Synthetic (no fake pulse).
    fn supports_aux_widgets(&self) -> bool {
        false
    }

    fn session_markers(&self) -> &[crate::markers::MarkerEvent] {
        &[]
    }

    fn playhead_sample(&self) -> Option<usize> {
        None
    }

    /// Left-column Time Series label. Override for montage names; default is `Ch N`.
    fn channel_label(&self, logical: usize) -> String {
        format!("Ch {}", logical + 1)
    }
}

/// Copy EXG columns out of a full BrainFlow row (which also holds index, timestamp, accel, …).
pub fn extract_exg(row: &[f64], exg_channels: &[usize]) -> Vec<f64> {
    exg_channels
        .iter()
        .map(|&i| row.get(i).copied().unwrap_or(0.0))
        .collect()
}

/// Samples appended by the most recent `DataSource::update()` call (filtered display).
pub fn recent_rows(source: &dyn DataSource) -> Vec<Vec<f64>> {
    let n = source.recent_samples_delivered();
    if n == 0 {
        Vec::new()
    } else {
        source.get_data(n)
    }
}

/// Unfiltered samples from the most recent `update()` (recording).
pub fn recent_raw_rows(source: &dyn DataSource) -> Vec<Vec<f64>> {
    let n = source.recent_samples_delivered();
    if n == 0 {
        Vec::new()
    } else {
        source.get_raw_data(n)
    }
}

#[cfg(test)]
mod tests {
    use super::extract_exg;

    #[test]
    fn extract_exg_skips_non_exg_columns() {
        // Typical Cyton-style row: sample index, 8 EXG, then extras.
        let row = vec![42.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 0.1, 0.2];
        let exg = vec![1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            extract_exg(&row, &exg),
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
        );
    }

    #[test]
    fn extract_exg_missing_columns_are_zero() {
        let row = vec![1.0];
        let exg = vec![0, 5];
        assert_eq!(extract_exg(&row, &exg), vec![1.0, 0.0]);
    }
}

/// Errors that can occur when talking to a board (BrainFlow or otherwise).
#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("BrainFlow error: {0}")]
    BrainFlow(String),

    #[error("Board is not initialized")]
    NotInitialized,

    #[error("I/O or device error: {0}")]
    Io(String),
}

// Re-export the concrete implementations (Phase 7: used via direct paths in app.rs for Playback
// to avoid "unused import" warnings while keeping the symbols available for future direct use).
#[allow(unused_imports)]
pub use playback::PlaybackBoard;
#[allow(unused_imports)]
pub use synthetic::SyntheticBoard;
