//! FilterSettings — per-channel filtering configuration.
//!
//! This is the Rust equivalent of the original Java `FilterSettings.pde`.
//! For now (Option B) we support:
//! - Notch filter (50/60 Hz) per channel
//! - Bandpass filter per channel

use brainflow::NoiseTypes;

#[derive(Clone, Debug)]
pub struct ChannelFilter {
    pub notch_enabled: bool,
    pub notch_type: NoiseTypes,

    pub bandpass_enabled: bool,
    pub bandpass_low: f64,
    pub bandpass_high: f64,
}

impl Default for ChannelFilter {
    fn default() -> Self {
        Self {
            notch_enabled: true,
            notch_type: NoiseTypes::FiftyAndSixty,
            bandpass_enabled: true,
            bandpass_low: 1.0,
            bandpass_high: 50.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FilterSettings {
    pub channels: Vec<ChannelFilter>,
}

impl FilterSettings {
    pub fn new(num_channels: usize) -> Self {
        Self {
            channels: vec![ChannelFilter::default(); num_channels],
        }
    }

    pub fn set_notch(&mut self, channel: usize, enabled: bool, noise_type: NoiseTypes) {
        if let Some(ch) = self.channels.get_mut(channel) {
            ch.notch_enabled = enabled;
            ch.notch_type = noise_type;
        }
    }

    pub fn set_bandpass(&mut self, channel: usize, enabled: bool, low: f64, high: f64) {
        if let Some(ch) = self.channels.get_mut(channel) {
            ch.bandpass_enabled = enabled;
            ch.bandpass_low = low;
            ch.bandpass_high = high;
        }
    }
}

/// Java `GlobalEnvironmentalFilter`: 50 / 60 / 50+60 / None.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NotchMode {
    Fifty,
    Sixty,
    #[default]
    FiftyAndSixty,
    Off,
}

impl NotchMode {
    pub const ALL: [NotchMode; 4] = [
        NotchMode::FiftyAndSixty,
        NotchMode::Fifty,
        NotchMode::Sixty,
        NotchMode::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NotchMode::Fifty => "50 Hz",
            NotchMode::Sixty => "60 Hz",
            NotchMode::FiftyAndSixty => "50 + 60 Hz",
            NotchMode::Off => "None",
        }
    }

    pub fn to_brainflow(self) -> (bool, NoiseTypes) {
        match self {
            NotchMode::Off => (false, NoiseTypes::FiftyAndSixty),
            NotchMode::Fifty => (true, NoiseTypes::Fifty),
            NotchMode::Sixty => (true, NoiseTypes::Sixty),
            NotchMode::FiftyAndSixty => (true, NoiseTypes::FiftyAndSixty),
        }
    }

    pub fn from_channel(ch: &ChannelFilter) -> Self {
        if !ch.notch_enabled {
            return NotchMode::Off;
        }
        match ch.notch_type {
            NoiseTypes::Fifty => NotchMode::Fifty,
            NoiseTypes::Sixty => NotchMode::Sixty,
            NoiseTypes::FiftyAndSixty => NotchMode::FiftyAndSixty,
        }
    }

    pub fn from_legacy_enabled(enabled: bool) -> Self {
        if enabled {
            NotchMode::FiftyAndSixty
        } else {
            NotchMode::Off
        }
    }
}

/// Apply bandpass then notch (Java `DataProcessing.processChannel` order).
pub fn apply_exg_filter(series: &mut [f64], sample_rate: usize, filter: &ChannelFilter) {
    if series.is_empty() {
        return;
    }
    if filter.bandpass_enabled && series.len() > 10 {
        let _ = brainflow::data_filter::perform_bandpass(
            series,
            sample_rate,
            filter.bandpass_low,
            filter.bandpass_high,
            4,
            brainflow::FilterTypes::Butterworth,
            0.0,
        );
    }
    if filter.notch_enabled {
        let _ = brainflow::data_filter::remove_environmental_noise(
            series,
            sample_rate,
            filter.notch_type,
        );
    }
}

/// Java `dataBuff_len_sec = 20 + 2` — extra 2 s so IIR startup sits off a 20 s window.
pub const DISPLAY_BUFFER_SECONDS: usize = 22;

pub fn display_buffer_keep(sample_rate: usize) -> usize {
    (sample_rate * DISPLAY_BUFFER_SECONDS).max(2000)
}

/// Clone raw rows and IIR EXG columns (Java `dataProcessingFilteredBuffer`).
pub fn rebuild_filtered_display(
    raw: &[Vec<f64>],
    exg_channels: &[usize],
    sample_rate: usize,
    settings: &FilterSettings,
) -> Vec<Vec<f64>> {
    let mut out = raw.to_vec();
    filter_exg_rows(&mut out, exg_channels, sample_rate, settings);
    out
}

/// Append unfiltered packets to the ring, then filter the whole window.
pub fn append_raw_and_filter(
    ring: &mut Vec<Vec<f64>>,
    new_rows: Vec<Vec<f64>>,
    max_keep: usize,
    exg_channels: &[usize],
    sample_rate: usize,
    settings: &FilterSettings,
) -> Vec<Vec<f64>> {
    if !new_rows.is_empty() {
        let total = ring.len() + new_rows.len();
        if total > max_keep {
            let excess = total - max_keep;
            if excess >= ring.len() {
                ring.clear();
            } else {
                ring.drain(0..excess);
            }
        }
        ring.extend(new_rows);
    }
    rebuild_filtered_display(ring, exg_channels, sample_rate, settings)
}

/// Filter EXG columns of a row-major buffer (Java: whole display buffer, not per-packet).
pub fn filter_exg_rows(
    rows: &mut [Vec<f64>],
    exg_channels: &[usize],
    sample_rate: usize,
    settings: &FilterSettings,
) {
    if rows.is_empty() {
        return;
    }
    let n_cols = rows[0].len();
    for (logical, &board_ch) in exg_channels.iter().enumerate() {
        if board_ch >= n_cols {
            continue;
        }
        let mut col: Vec<f64> = rows
            .iter()
            .map(|r| r.get(board_ch).copied().unwrap_or(0.0))
            .collect();
        let filt = settings.channels.get(logical).cloned().unwrap_or_default();
        apply_exg_filter(&mut col, sample_rate, &filt);
        for (row, v) in rows.iter_mut().zip(col) {
            if board_ch < row.len() {
                row[board_ch] = v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brainflow::NoiseTypes;

    #[test]
    fn java_labels_match_filter_enums() {
        assert_eq!(NotchMode::Fifty.label(), "50 Hz");
        assert_eq!(NotchMode::Sixty.label(), "60 Hz");
        assert_eq!(NotchMode::FiftyAndSixty.label(), "50 + 60 Hz");
        assert_eq!(NotchMode::Off.label(), "None");
    }

    #[test]
    fn off_disables_brainflow_notch() {
        let (enabled, _) = NotchMode::Off.to_brainflow();
        assert!(!enabled);
    }

    #[test]
    fn fifty_sixty_and_both_map_to_brainflow_noise_types() {
        assert_eq!(NotchMode::Fifty.to_brainflow(), (true, NoiseTypes::Fifty));
        assert_eq!(NotchMode::Sixty.to_brainflow(), (true, NoiseTypes::Sixty));
        assert_eq!(
            NotchMode::FiftyAndSixty.to_brainflow(),
            (true, NoiseTypes::FiftyAndSixty)
        );
    }

    #[test]
    fn from_channel_round_trips() {
        for mode in NotchMode::ALL {
            let (enabled, noise) = mode.to_brainflow();
            let ch = ChannelFilter {
                notch_enabled: enabled,
                notch_type: noise,
                ..ChannelFilter::default()
            };
            assert_eq!(NotchMode::from_channel(&ch), mode);
        }
    }

    #[test]
    fn old_bool_config_maps_to_java_defaults() {
        assert_eq!(
            NotchMode::from_legacy_enabled(true),
            NotchMode::FiftyAndSixty
        );
        assert_eq!(NotchMode::from_legacy_enabled(false), NotchMode::Off);
    }

    #[test]
    fn display_buffer_matches_java_20_plus_2_seconds() {
        assert_eq!(DISPLAY_BUFFER_SECONDS, 22);
        assert_eq!(display_buffer_keep(250), 250 * 22);
    }

    fn blink_like(sr: usize, seconds: f64, amp: f64, hz: f64) -> Vec<f64> {
        let n = (sr as f64 * seconds) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / sr as f64).sin())
            .collect()
    }

    fn peak(xs: &[f64]) -> f64 {
        xs.iter().fold(0.0_f64, |a, &x| a.max(x.abs()))
    }

    #[test]
    fn slow_blink_survives_full_window_bandpass() {
        let sr = 250usize;
        let sig = blink_like(sr, 2.0, 200.0, 2.0);
        let filt = ChannelFilter {
            notch_enabled: false,
            notch_type: NoiseTypes::FiftyAndSixty,
            bandpass_enabled: true,
            bandpass_low: 1.0,
            bandpass_high: 50.0,
        };
        let mut full = sig.clone();
        apply_exg_filter(&mut full, sr, &filt);
        assert!(
            peak(&full) > 80.0,
            "2 Hz blink-like wave should remain large after 1–50 Hz on a 2s window, peak={}",
            peak(&full)
        );
    }

    #[test]
    fn tiny_packets_skip_bandpass_and_leave_dc_offset() {
        let sr = 250usize;
        let n = sr * 2;
        let sig: Vec<f64> = (0..n)
            .map(|i| {
                5000.0 + 200.0 * (2.0 * std::f64::consts::PI * 2.0 * i as f64 / sr as f64).sin()
            })
            .collect();
        let filt = ChannelFilter {
            notch_enabled: false,
            notch_type: NoiseTypes::FiftyAndSixty,
            bandpass_enabled: true,
            bandpass_low: 1.0,
            bandpass_high: 50.0,
        };
        let mut chunked = Vec::new();
        for chunk in sig.chunks(8) {
            let mut c = chunk.to_vec();
            apply_exg_filter(&mut c, sr, &filt);
            chunked.extend(c);
        }
        assert!(
            peak(&chunked) > 4000.0,
            "8-sample packets skip bandpass (len≤10), so Cyton DC remains and swamps ±200 µV blinks, peak={}",
            peak(&chunked)
        );
    }

    fn blink_settings() -> FilterSettings {
        let mut settings = FilterSettings::new(1);
        settings.channels[0] = ChannelFilter {
            notch_enabled: false,
            notch_type: NoiseTypes::FiftyAndSixty,
            bandpass_enabled: true,
            bandpass_low: 1.0,
            bandpass_high: 50.0,
        };
        settings
    }

    #[test]
    fn full_window_bandpass_drops_dc_and_keeps_blink() {
        let sr = 250usize;
        let n = sr * 2;
        let sig: Vec<f64> = (0..n)
            .map(|i| {
                5000.0 + 200.0 * (2.0 * std::f64::consts::PI * 2.0 * i as f64 / sr as f64).sin()
            })
            .collect();
        let mut rows: Vec<Vec<f64>> = sig.iter().map(|&v| vec![v]).collect();
        filter_exg_rows(&mut rows, &[0], sr, &blink_settings());
        let tail: Vec<f64> = rows.iter().skip(sr).map(|r| r[0]).collect();
        let p = peak(&tail);
        assert!(
            p > 80.0 && p < 800.0,
            "1–50 Hz on 2s should keep 2 Hz blink and drop 5 mV DC, peak={}",
            p
        );
    }

    #[test]
    fn live_ring_keeps_blink_when_raw_packets_are_filtered_as_one_window() {
        let sr = 250usize;
        let sig = blink_like(sr, 2.0, 200.0, 2.0);
        let settings = blink_settings();
        let mut ring = Vec::new();
        let mut display = Vec::new();
        for chunk in sig.chunks(8) {
            let rows: Vec<Vec<f64>> = chunk.iter().map(|&v| vec![v]).collect();
            display = append_raw_and_filter(&mut ring, rows, 8_000, &[0], sr, &settings);
        }
        let filtered: Vec<f64> = display.iter().map(|r| r[0]).collect();
        let raw: Vec<f64> = ring.iter().map(|r| r[0]).collect();
        assert!(
            peak(&raw) > 199.0,
            "ring must store unfiltered samples, peak={}",
            peak(&raw)
        );
        assert!(
            peak(&filtered) > 80.0,
            "Java-style full-window IIR must keep a 2 Hz blink, peak={}",
            peak(&filtered)
        );
    }

    #[test]
    fn filter_exg_rows_leaves_non_exg_columns_alone() {
        let mut rows = vec![vec![99.0, 5.0], vec![99.0, 5.0], vec![99.0, 5.0]];
        let settings = blink_settings();
        filter_exg_rows(&mut rows, &[1], 250, &settings);
        assert_eq!(rows[0][0], 99.0);
        assert_eq!(rows[1][0], 99.0);
    }

    fn live_sixty_notch_1_50() -> ChannelFilter {
        ChannelFilter {
            notch_enabled: true,
            notch_type: NoiseTypes::Sixty,
            bandpass_enabled: true,
            bandpass_low: 1.0,
            bandpass_high: 50.0,
        }
    }

    fn tone(sr: usize, seconds: usize, amp: f64, hz: f64) -> Vec<f64> {
        let n = sr * seconds;
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / sr as f64).sin())
            .collect()
    }

    fn peak_in(xs: &[f64], start: usize, len: usize) -> f64 {
        let end = (start + len).min(xs.len());
        peak(&xs[start.min(xs.len())..end])
    }

    #[test]
    fn sixty_hz_sine_is_removed_by_live_sixty_notch() {
        let sr = 250usize;
        let mut sig = tone(sr, 4, 80.0, 60.0);
        apply_exg_filter(&mut sig, sr, &live_sixty_notch_1_50());
        let mid = peak_in(&sig, sr, sr);
        assert!(
            mid < 5.0,
            "60 Hz mains should be crushed by Notch 60, peak={mid}"
        );
    }

    #[test]
    fn fifty_hz_line_noise_survives_sixty_notch_and_bp_1_50() {
        let sr = 250usize;
        let mut sig = tone(sr, 4, 80.0, 50.0);
        apply_exg_filter(&mut sig, sr, &live_sixty_notch_1_50());
        let mid = peak_in(&sig, sr, sr);
        assert!(
            mid > 20.0,
            "50 Hz mains should still be large with Notch 60 / BP 1-50, peak={mid}"
        );
    }

    #[test]
    fn fifty_and_sixty_notch_removes_fifty_hz_line_noise() {
        let sr = 250usize;
        let mut sig = tone(sr, 4, 80.0, 50.0);
        let filt = ChannelFilter {
            notch_enabled: true,
            notch_type: NoiseTypes::FiftyAndSixty,
            bandpass_enabled: true,
            bandpass_low: 1.0,
            bandpass_high: 50.0,
        };
        apply_exg_filter(&mut sig, sr, &filt);
        let mid = peak_in(&sig, sr, sr);
        assert!(
            mid < 5.0,
            "Java-default 50+60 notch should crush 50 Hz mains, peak={mid}"
        );
    }

    #[test]
    fn notch_sixty_digs_a_hole_at_sixty_in_the_fft() {
        let sr = 250usize;
        let n = sr * 4;
        let mut sig: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / sr as f64;
                80.0 * (2.0 * std::f64::consts::PI * 50.0 * t).sin()
                    + 80.0 * (2.0 * std::f64::consts::PI * 60.0 * t).sin()
            })
            .collect();
        apply_exg_filter(&mut sig, sr, &live_sixty_notch_1_50());
        let nfft = crate::fft::nfft_safe(sr as i32);
        let tail = &sig[sig.len() - nfft..];
        let (freqs, mags) = crate::fft::fft_display_uv(tail, sr as f64, 100.0);
        let at_50 = crate::fft::bin_near(&freqs, &mags, 50.0);
        let at_60 = crate::fft::bin_near(&freqs, &mags, 60.0);
        assert!(
            at_50 > 4.0 * at_60.max(1e-9),
            "Notch 60 must cut 60 Hz in the FFT while 50 Hz remains, 50={at_50} 60={at_60}"
        );
    }

    #[test]
    fn spliced_packet_gap_of_a_small_sine_does_not_invent_a_burst() {
        let sr = 250usize;
        let drop_at = sr * 2;
        let drop_n = 8;
        let n = sr * 4;
        let mut spliced: Vec<f64> = (0..n)
            .filter(|&i| i < drop_at || i >= drop_at + drop_n)
            .map(|i| 10.0 * (2.0 * std::f64::consts::PI * 10.0 * i as f64 / sr as f64).sin())
            .collect();
        apply_exg_filter(&mut spliced, sr, &live_sixty_notch_1_50());
        let join_peak = peak_in(&spliced, drop_at.saturating_sub(40), 80);
        let clean_peak = peak_in(&spliced, sr, sr / 2);
        assert!(
            join_peak < 3.0 * clean_peak.max(1.0),
            "an 8-sample splice of a 10 µV sine is not the all-channel burst, clean={clean_peak} join={join_peak}"
        );
    }
}
