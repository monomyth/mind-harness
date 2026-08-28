//! Playback board — implements DataSource by reading a previously recorded
//! OpenBCI ODF .txt file (the exact format produced by our DataLogger when
//! recording in ODF mode).
//!
//! This enables the magical "record a session with Focus + markers + networking
//! → End Session → immediately play it back" workflow that is the gold standard
//! for validating the full experiment loop in Phase 7.
//!
//! The parser is deliberately tolerant:
//! - Supports both the Rust port header ("OpenBCI Data Format (Rust port)")
//! - And the classic Java GUI ODF header ("%OpenBCI Raw EXG Data")
//! - Extracts sample rate and channel count
//! - Treats the first numeric column after optional "timestamp" header as ch0
//!
//! During playback we advance a playhead in real (wall-clock) time, scaled by
//! a user-controllable speed (0.25× … 4×). All widgets (TimeSeries, Focus ML+audio,
//! Networking output, Console logging, Marker sending) continue to work exactly
//! as they do with live hardware or Synthetic.
//!
//! plan.md Phase 7 step 4

use crate::board::{BoardError, DataSource};
use crate::filter_settings::FilterSettings;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;

pub struct PlaybackBoard {
    samples: Vec<Vec<f64>>, // each inner vec is one full row (as written by logger)
    exg_channels: Vec<usize>,
    accel_channels: Vec<usize>,
    sample_rate: i32,
    playhead: usize, // current sample index we are "at"
    last_wall_time: Instant,
    speed: f32, // playback multiplier (1.0 = real time)
    paused: bool,
    filename: String,
    total_duration_sec: f64,
    /// Phase 7: for accurate packet loss / delivered count in WPacketLoss sparkline
    last_delivered: usize,
    filter_settings: FilterSettings,
    /// Filtered copy of `samples` (EXG columns only). Rebuilt when notch/bandpass change.
    filtered: Vec<Vec<f64>>,
    filter_dirty: bool,
}

impl PlaybackBoard {
    /// Load and parse an ODF .txt recording (Rust or Java GUI format).
    pub fn from_file(path: &std::path::Path) -> Result<Self, BoardError> {
        let file = File::open(path).map_err(|e| BoardError::Io(e.to_string()))?;
        let reader = BufReader::new(file);

        let mut sample_rate: i32 = 250;
        let mut n_channels: usize = 8;
        let mut data_start = false;
        let mut skip_first_column = false;
        let mut samples: Vec<Vec<f64>> = Vec::new();
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        for line in reader.lines() {
            let line = line.map_err(|e| BoardError::Io(e.to_string()))?;
            let trimmed = line.trim();

            if trimmed.starts_with("OpenBCI") || trimmed.starts_with("%OpenBCI") {
                continue;
            }
            if trimmed.starts_with("Sample Rate:")
                || trimmed.starts_with("%Sample Rate")
                || trimmed.contains("Sample Rate =")
                || trimmed.contains("Sample Rate:")
            {
                if let Some(s) = trimmed
                    .split(':')
                    .nth(1)
                    .or_else(|| trimmed.split('=').nth(1))
                {
                    if let Ok(sr) = s.split_whitespace().next().unwrap_or("250").parse() {
                        sample_rate = sr;
                    }
                    if let Some(chs) = s.split("Channels:").nth(1) {
                        if let Ok(n) = chs.split_whitespace().next().unwrap_or("8").parse() {
                            n_channels = n;
                        }
                    }
                }
                continue;
            }
            if trimmed.starts_with("Channels:") || trimmed.contains("Number of channels") {
                if let Some(s) = trimmed
                    .split('=')
                    .nth(1)
                    .or_else(|| trimmed.split(':').nth(1))
                {
                    if let Ok(n) = s.split_whitespace().next().unwrap_or("8").parse() {
                        n_channels = n;
                    }
                }
                continue;
            }
            if trimmed.starts_with("timestamp")
                || trimmed.starts_with("EXG Channel")
                || trimmed.contains("ch0")
                || trimmed.starts_with("Sample Index")
                || trimmed.contains("EXG Channel")
            {
                // Java ODF: "Sample Index, EXG Channel 0, ..." — drop the index column.
                // Older Rust header: "timestamp,ch0,..." — drop timestamp.
                let lower = trimmed.to_ascii_lowercase();
                skip_first_column =
                    lower.starts_with("timestamp") || lower.starts_with("sample index");
                data_start = true;
                continue;
            }
            if !data_start {
                continue;
            }

            // Data row: comma or space separated floats
            let parts: Vec<&str> = trimmed
                .split(&[',', ' ', '\t'][..])
                .filter(|s| !s.is_empty())
                .collect();
            if parts.len() < 2 {
                continue;
            }

            let mut row: Vec<f64> = parts.iter().filter_map(|p| p.parse::<f64>().ok()).collect();
            if skip_first_column && !row.is_empty() {
                row.remove(0);
            }

            if row.len() >= 2 {
                samples.push(row);
            }
        }

        if samples.is_empty() {
            return Err(BoardError::Io(
                "No numeric data rows found in recording".into(),
            ));
        }

        // Heuristic for Playback roundtrip fidelity (plan.md Phase 7):
        // BrainFlow board rows start with EXG channels at indices 0.. (get_exg_channels returns [0,1,..] for synthetic/Cyton).
        // The ODF writer records the full rows as-is; we reconstruct exg_channels starting at 0 so that
        // WTimeSeries, WFocus (ML + proxy using exg_channels() + get_avg..), BandPower etc. see identical
        // data layout during replay as they did live. This makes the "record → immediate Playback" magical.
        let row_len = samples[0].len();
        let n_exg = n_channels.min(row_len).max(1);
        let exg_channels: Vec<usize> = (0..n_exg).collect();
        let accel_channels: Vec<usize> = if row_len > n_exg {
            (n_exg..(n_exg + 3).min(row_len)).collect()
        } else {
            vec![]
        };

        let total_samples = samples.len();
        let total_duration_sec = total_samples as f64 / sample_rate as f64;

        let n_exg_for_filters = n_exg;
        let mut board = Self {
            samples,
            exg_channels,
            accel_channels,
            sample_rate,
            playhead: 0,
            last_wall_time: Instant::now(),
            speed: 1.0,
            paused: false,
            filename,
            total_duration_sec,
            last_delivered: 0,
            filter_settings: FilterSettings::new(n_exg_for_filters),
            filtered: vec![],
            filter_dirty: true,
        };
        board.apply_pending_filters();
        Ok(board)
    }

    fn rebuild_filtered(&mut self) {
        self.filtered = crate::filter_settings::rebuild_filtered_display(
            &self.samples,
            &self.exg_channels,
            self.sample_rate as usize,
            &self.filter_settings,
        );
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.1, 8.0);
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            self.last_wall_time = Instant::now();
        }
    }

    pub fn seek_to_fraction(&mut self, frac: f32) {
        let target = ((frac.clamp(0.0, 1.0) as f64) * self.samples.len() as f64) as usize;
        self.playhead = target.min(self.samples.len().saturating_sub(1));
        self.last_wall_time = Instant::now();
    }

    /// Phase 7: direct progress (0..1); UI currently uses the trait playback_progress().
    /// Kept for potential direct PlaybackBoard consumers / debug.
    #[allow(dead_code)]
    pub fn progress(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        (self.playhead as f32 / self.samples.len() as f32).min(1.0)
    }

    /// Phase 7: introspection retained for future "now playing" status / tooltip.
    /// Current roundtrip UI computes duration from sample count; these are vestigial but kept.
    #[allow(dead_code)]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    #[allow(dead_code)]
    pub fn total_duration(&self) -> f64 {
        self.total_duration_sec
    }

    /// Current playback speed multiplier (for UI display / trait)
    pub fn speed(&self) -> f32 {
        self.speed
    }

    // Phase 7 WPacketLoss support — exposed via the DataSource defaulted method below
    #[allow(dead_code)]
    pub fn last_delivered(&self) -> usize {
        self.last_delivered
    }

    #[cfg(test)]
    fn parsed_rows(&self) -> &[Vec<f64>] {
        &self.samples
    }
}

impl DataSource for PlaybackBoard {
    fn initialize(&mut self) -> Result<(), BoardError> {
        self.playhead = 0;
        self.last_wall_time = Instant::now();
        self.paused = false;
        self.last_delivered = 0;
        Ok(())
    }

    fn uninitialize(&mut self) -> Result<(), BoardError> {
        Ok(())
    }

    fn update(&mut self) {
        self.apply_pending_filters();
        if self.paused || self.samples.is_empty() {
            return;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_wall_time).as_secs_f64();
        self.last_wall_time = now;

        // Advance playhead by real-time * speed * sample_rate
        let advance = (elapsed * self.speed as f64 * self.sample_rate as f64) as usize;
        let prev = self.playhead;
        if advance > 0 {
            self.playhead = (self.playhead + advance).min(self.samples.len().saturating_sub(1));
        }
        self.last_delivered = self.playhead.saturating_sub(prev);
    }

    fn start_streaming(&mut self) -> Result<(), BoardError> {
        self.paused = false;
        self.last_wall_time = Instant::now();
        Ok(())
    }

    fn stop_streaming(&mut self) -> Result<(), BoardError> {
        self.paused = true;
        Ok(())
    }

    fn is_streaming(&self) -> bool {
        !self.paused
    }

    fn total_channel_count(&self) -> usize {
        self.samples.first().map(|r| r.len()).unwrap_or(8)
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
        let src = if self.filtered.is_empty() {
            &self.samples
        } else {
            &self.filtered
        };
        playback_tail(src, self.playhead, max_samples)
    }

    fn get_raw_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        playback_tail(&self.samples, self.playhead, max_samples)
    }

    fn get_frame_data(&self) -> Vec<Vec<f64>> {
        let src = if self.filtered.is_empty() {
            &self.samples
        } else {
            &self.filtered
        };
        if src.is_empty() || self.playhead >= src.len() {
            return vec![];
        }
        vec![src[self.playhead].clone()]
    }

    fn name(&self) -> &str {
        "Playback"
    }

    // Phase 7 trait extensions for UI time cursor / speed
    fn playback_progress(&self) -> Option<(usize, usize)> {
        Some((self.playhead, self.samples.len()))
    }
    fn set_playback_speed(&mut self, speed: f32) {
        self.set_speed(speed);
    }
    fn playback_speed(&self) -> Option<f32> {
        Some(self.speed())
    }

    // Phase 7: wire the new control methods so the status bar UI can drive them
    fn toggle_playback_pause(&mut self) {
        self.toggle_pause();
    }
    fn seek_to_fraction(&mut self, frac: f32) {
        self.seek_to_fraction(frac);
    }

    fn recent_samples_delivered(&self) -> usize {
        self.last_delivered
    }

    // Playback: impedance is simulated so the UI is fully usable during "magical" record→play roundtrips.
    fn supports_impedance(&self) -> bool {
        true
    }
    fn start_impedance_test(&mut self, _channels: &[usize]) -> Result<(), BoardError> {
        Ok(())
    }
    fn stop_impedance_test(&mut self) -> Result<(), BoardError> {
        Ok(())
    }
    fn get_impedance(&self) -> Vec<Option<f64>> {
        // Simulated only — labelled in the Impedance widget.
        let n = self.exg_channels.len();
        (0..n)
            .map(|i| Some(3.0 + ((i as f64) * 0.7) + ((self.playhead / 20) % 7) as f64 * 0.3))
            .collect()
    }

    fn impedance_is_simulated(&self) -> bool {
        true
    }

    fn get_filter_settings(&self) -> Option<&FilterSettings> {
        Some(&self.filter_settings)
    }

    fn set_notch_filter(
        &mut self,
        channel: usize,
        enabled: bool,
        noise_type: brainflow::NoiseTypes,
    ) {
        self.filter_settings.set_notch(channel, enabled, noise_type);
        self.filter_dirty = true;
    }

    fn set_bandpass_filter(&mut self, channel: usize, enabled: bool, low: f64, high: f64) {
        self.filter_settings.set_bandpass(channel, enabled, low, high);
        self.filter_dirty = true;
    }

    fn apply_pending_filters(&mut self) {
        if self.filter_dirty {
            self.rebuild_filtered();
            self.filter_dirty = false;
        }
    }
}

fn playback_tail(src: &[Vec<f64>], playhead: usize, max_samples: usize) -> Vec<Vec<f64>> {
    if src.is_empty() {
        return vec![];
    }
    let end = playhead + 1;
    let start = end.saturating_sub(max_samples);
    src[start..end.min(src.len())].to_vec()
}

#[cfg(test)]
mod tests {
    use super::PlaybackBoard;
    use crate::board::DataSource;
    use std::io::Write;

    #[test]
    fn rust_odf_roundtrip_first_column_is_ch0() {
        let dir = std::env::temp_dir();
        let path = dir.join("openbci_playback_test_rust.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "OpenBCI Data Format (Rust port) - test").unwrap();
        writeln!(f, "Sample Rate: 250 Hz, Channels: 2").unwrap();
        writeln!(f, "ch0,ch1").unwrap();
        writeln!(f, "1.5,2.5").unwrap();
        writeln!(f, "3.5,4.5").unwrap();
        drop(f);

        let pb = PlaybackBoard::from_file(&path).unwrap();
        assert_eq!(pb.sample_rate(), 250);
        assert_eq!(pb.exg_channels(), &[0, 1]);
        assert_eq!(pb.parsed_rows()[0], vec![1.5, 2.5]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn timestamp_header_skips_first_column() {
        let dir = std::env::temp_dir();
        let path = dir.join("openbci_playback_test_ts.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "OpenBCI Data Format (Rust port)").unwrap();
        writeln!(f, "Sample Rate: 250 Hz, Channels: 2").unwrap();
        writeln!(f, "timestamp,ch0,ch1").unwrap();
        writeln!(f, "0.0,10.0,20.0").unwrap();
        drop(f);

        let pb = PlaybackBoard::from_file(&path).unwrap();
        assert_eq!(pb.parsed_rows()[0], vec![10.0, 20.0]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn java_odf_skips_sample_index_column() {
        let dir = std::env::temp_dir();
        let path = dir.join("openbci_playback_test_java.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "%OpenBCI Raw EXG Data").unwrap();
        writeln!(f, "%Number of channels = 2").unwrap();
        writeln!(f, "%Sample Rate = 250 Hz").unwrap();
        writeln!(f, "Sample Index, EXG Channel 0, EXG Channel 1").unwrap();
        writeln!(f, "0, 1.0, 2.0").unwrap();
        writeln!(f, "1, 3.0, 4.0").unwrap();
        drop(f);

        let pb = PlaybackBoard::from_file(&path).unwrap();
        assert_eq!(pb.sample_rate(), 250);
        assert_eq!(pb.parsed_rows()[0], vec![1.0, 2.0]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn playback_exposes_filter_settings_like_live_boards() {
        let dir = std::env::temp_dir();
        let path = dir.join("openbci_playback_test_filters.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "OpenBCI Data Format (Rust port)").unwrap();
        writeln!(f, "Sample Rate: 250 Hz, Channels: 2").unwrap();
        writeln!(f, "ch0,ch1").unwrap();
        writeln!(f, "1.0,2.0").unwrap();
        drop(f);

        let mut pb = PlaybackBoard::from_file(&path).unwrap();
        assert!(pb.get_filter_settings().is_some());
        pb.set_notch_filter(0, true, brainflow::NoiseTypes::Fifty);
        pb.set_notch_filter(1, true, brainflow::NoiseTypes::Fifty);
        pb.set_bandpass_filter(0, false, 1.0, 50.0);
        pb.set_bandpass_filter(1, false, 1.0, 50.0);
        pb.apply_pending_filters();
        assert_eq!(
            crate::filter_settings::NotchMode::from_channel(
                &pb.get_filter_settings().unwrap().channels[0]
            ),
            crate::filter_settings::NotchMode::Fifty
        );
        pb.set_notch_filter(0, false, brainflow::NoiseTypes::Fifty);
        pb.set_notch_filter(1, false, brainflow::NoiseTypes::Fifty);
        pb.apply_pending_filters();
        assert_eq!(pb.get_data(1)[0], vec![1.0, 2.0]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn playback_get_raw_data_keeps_dc_that_bandpass_removes() {
        let dir = std::env::temp_dir();
        let path = dir.join("openbci_playback_test_raw_vs_filt.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "OpenBCI Data Format (Rust port)").unwrap();
        writeln!(f, "Sample Rate: 250 Hz, Channels: 2").unwrap();
        writeln!(f, "ch0,ch1").unwrap();
        for _ in 0..250 {
            writeln!(f, "100.0,0.0").unwrap();
        }
        drop(f);

        let mut pb = PlaybackBoard::from_file(&path).unwrap();
        pb.seek_to_fraction(1.0);
        let raw = pb.get_raw_data(250);
        let filt = pb.get_data(250);
        let last_raw = raw.last().unwrap()[0];
        let last_filt = filt.last().unwrap()[0];
        assert!(
            (last_raw - 100.0).abs() < 0.01,
            "raw must stay at DC, last={}",
            last_raw
        );
        assert!(
            last_filt.abs() < 15.0,
            "1–50 Hz display buffer should settle near 0 on DC, last={}",
            last_filt
        );
        let _ = std::fs::remove_file(path);
    }
}
