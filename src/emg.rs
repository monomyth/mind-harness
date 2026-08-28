//! EMG envelope + adaptive thresholds — Java `EmgSettingsValues.process`.
//!
//! Shared by the EMG widget, EMG Joystick, and (later) networking EMG output.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmgWindow {
    Hundredth,
    Tenth,
    FifteenHundredths,
    Quarter,
    Half,
    ThreeQuarters,
    One,
    Two,
}

impl EmgWindow {
    pub const ALL: [EmgWindow; 8] = [
        EmgWindow::Hundredth,
        EmgWindow::Tenth,
        EmgWindow::FifteenHundredths,
        EmgWindow::Quarter,
        EmgWindow::Half,
        EmgWindow::ThreeQuarters,
        EmgWindow::One,
        EmgWindow::Two,
    ];

    pub fn seconds(self) -> f64 {
        match self {
            EmgWindow::Hundredth => 0.01,
            EmgWindow::Tenth => 0.1,
            EmgWindow::FifteenHundredths => 0.15,
            EmgWindow::Quarter => 0.25,
            EmgWindow::Half => 0.5,
            EmgWindow::ThreeQuarters => 0.75,
            EmgWindow::One => 1.0,
            EmgWindow::Two => 2.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EmgWindow::Hundredth => "0.01 s",
            EmgWindow::Tenth => "0.1 s",
            EmgWindow::FifteenHundredths => "0.15 s",
            EmgWindow::Quarter => "0.25 s",
            EmgWindow::Half => "0.5 s",
            EmgWindow::ThreeQuarters => "0.75 s",
            EmgWindow::One => "1.0 s",
            EmgWindow::Two => "2.0 s",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmgUvLimit {
    Fifty,
    OneHundred,
    TwoHundred,
    FourHundred,
}

impl EmgUvLimit {
    pub const ALL: [EmgUvLimit; 4] = [
        EmgUvLimit::Fifty,
        EmgUvLimit::OneHundred,
        EmgUvLimit::TwoHundred,
        EmgUvLimit::FourHundred,
    ];

    pub fn uv(self) -> f64 {
        match self {
            EmgUvLimit::Fifty => 50.0,
            EmgUvLimit::OneHundred => 100.0,
            EmgUvLimit::TwoHundred => 200.0,
            EmgUvLimit::FourHundred => 400.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EmgUvLimit::Fifty => "50 uV",
            EmgUvLimit::OneHundred => "100 uV",
            EmgUvLimit::TwoHundred => "200 uV",
            EmgUvLimit::FourHundred => "400 uV",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmgCreep(pub f64);

impl EmgCreep {
    pub const ALL_INC: [f64; 7] = [0.9, 0.95, 0.98, 0.99, 0.999, 0.9999, 0.99999];
}

pub const MIN_DELTA_UV: [f64; 8] = [2.0, 4.0, 6.0, 8.0, 10.0, 20.0, 40.0, 80.0];
pub const LOWER_MIN_UV: [f64; 10] = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 15.0, 20.0, 30.0, 40.0];

#[derive(Clone, Copy, Debug)]
pub struct EmgChannelSettings {
    pub window: EmgWindow,
    pub uv_limit: EmgUvLimit,
    pub creep_increasing: f64,
    pub creep_decreasing: f64,
    pub minimum_delta_uv: f64,
    pub lower_threshold_minimum: f64,
}

impl Default for EmgChannelSettings {
    fn default() -> Self {
        Self {
            window: EmgWindow::One,
            uv_limit: EmgUvLimit::TwoHundred,
            creep_increasing: 0.9,
            creep_decreasing: 0.99999,
            minimum_delta_uv: 10.0,
            lower_threshold_minimum: 6.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EmgChannelState {
    pub settings: EmgChannelSettings,
    pub output_normalized: f64,
    pub upper_threshold: f64,
    pub lower_threshold: f64,
    pub average_uv: f64,
}

impl Default for EmgChannelState {
    fn default() -> Self {
        Self {
            settings: EmgChannelSettings::default(),
            output_normalized: 0.0,
            upper_threshold: 25.0,
            lower_threshold: 0.0,
            average_uv: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EmgProcessor {
    pub channels: Vec<EmgChannelState>,
}

impl EmgProcessor {
    pub fn new(n: usize) -> Self {
        Self {
            channels: vec![EmgChannelState::default(); n],
        }
    }

    pub fn ensure_channels(&mut self, n: usize) {
        if self.channels.len() == n {
            return;
        }
        self.channels.resize(n, EmgChannelState::default());
    }

    pub fn reset(&mut self) {
        for ch in &mut self.channels {
            let settings = ch.settings;
            *ch = EmgChannelState {
                settings,
                ..EmgChannelState::default()
            };
        }
    }

    /// Java `EmgSettingsValues.process` on filtered EXG columns (row-major board rows).
    pub fn process(&mut self, rows: &[Vec<f64>], exg: &[usize], sample_rate: i32) {
        let n = exg.len().min(self.channels.len());
        if n == 0 || sample_rate < 1 {
            return;
        }
        let sr = sample_rate as f64;
        for i in 0..n {
            let col = exg[i];
            let series: Vec<f64> = rows.iter().map(|r| r.get(col).copied().unwrap_or(0.0)).collect();
            process_channel(&mut self.channels[i], &series, sr);
        }
    }

    pub fn process_source(&mut self, source: &dyn crate::board::DataSource) {
        let exg = source.exg_channels();
        self.ensure_channels(exg.len());
        let sr = source.sample_rate();
        let keep = (sr.max(1) as usize * 2).max(32);
        let rows = source.get_data(keep);
        self.process(&rows, exg, sr);
    }
}

fn process_channel(ch: &mut EmgChannelState, series: &[f64], sample_rate: f64) {
    let s = ch.settings;
    let uv_limit = s.uv_limit.uv();
    let period = (sample_rate * s.window.seconds()).max(1.0);
    let n_want = (period as usize).max(1);
    let start = series.len().saturating_sub(n_want);
    let mut sum = 0.0;
    for &v in &series[start..] {
        let a = v.abs();
        sum += if a <= uv_limit { a } else { uv_limit };
    }
    // Java always divides by `averagePeriod` (sr * window), even if the buffer is short.
    ch.average_uv = sum / period;

    if ch.average_uv >= ch.upper_threshold && ch.average_uv <= uv_limit {
        ch.upper_threshold = ch.average_uv;
    }
    if ch.average_uv <= ch.lower_threshold {
        ch.lower_threshold = ch.average_uv;
    }
    if ch.upper_threshold >= ch.average_uv + s.minimum_delta_uv {
        ch.upper_threshold *= s.creep_increasing;
    }
    if ch.lower_threshold <= 1.0 {
        ch.lower_threshold = 1.0;
    }
    if ch.lower_threshold <= ch.average_uv {
        ch.lower_threshold *= 1.0 / s.creep_decreasing.max(1e-12);
    }
    if ch.lower_threshold < s.lower_threshold_minimum {
        ch.lower_threshold = s.lower_threshold_minimum;
    }
    if ch.upper_threshold <= ch.lower_threshold + s.minimum_delta_uv {
        ch.upper_threshold = ch.lower_threshold + s.minimum_delta_uv;
    }

    let span = ch.upper_threshold - ch.lower_threshold;
    ch.output_normalized = if span.abs() < 1e-12 {
        0.0
    } else {
        ((ch.average_uv - ch.lower_threshold) / span).max(0.0)
    };
}

/// Java `W_EMGJoystick.mapToUnitCircle` (uses the already-updated x when computing y).
pub fn map_to_unit_circle(mut x: f64, mut y: f64) -> (f64, f64) {
    x *= (1.0 - (y * y) / 2.0).max(0.0).sqrt();
    y *= (1.0 - (x * x) / 2.0).max(0.0).sqrt();
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn const_series(n: usize, v: f64) -> Vec<f64> {
        vec![v; n]
    }

    #[test]
    fn constant_under_limit_is_the_average() {
        let mut ch = EmgChannelState::default();
        process_channel(&mut ch, &const_series(250, 20.0), 250.0);
        assert!(
            (ch.average_uv - 20.0).abs() < 1e-9,
            "avg={}",
            ch.average_uv
        );
    }

    #[test]
    fn spikes_are_clipped_to_uv_limit_in_the_average() {
        let mut ch = EmgChannelState::default();
        ch.settings.uv_limit = EmgUvLimit::Fifty;
        process_channel(&mut ch, &const_series(250, 500.0), 250.0);
        assert!(
            (ch.average_uv - 50.0).abs() < 1e-9,
            "avg={}",
            ch.average_uv
        );
    }

    #[test]
    fn upper_snaps_up_to_a_rising_envelope() {
        let mut ch = EmgChannelState::default();
        ch.upper_threshold = 25.0;
        process_channel(&mut ch, &const_series(250, 40.0), 250.0);
        assert!(
            (ch.upper_threshold - 40.0).abs() < 1e-6 || ch.upper_threshold >= 40.0 - 1e-6,
            "upper={}",
            ch.upper_threshold
        );
    }

    #[test]
    fn output_is_zero_at_the_lower_threshold_and_positive_when_higher() {
        let mut quiet = EmgChannelState::default();
        quiet.lower_threshold = 6.0;
        quiet.upper_threshold = 16.0;
        process_channel(&mut quiet, &const_series(250, 6.0), 250.0);
        assert!(
            quiet.output_normalized < 0.15,
            "near lower should be small, n={}",
            quiet.output_normalized
        );

        let mut flexed = EmgChannelState::default();
        flexed.lower_threshold = 6.0;
        flexed.upper_threshold = 16.0;
        process_channel(&mut flexed, &const_series(250, 16.0), 250.0);
        assert!(
            flexed.output_normalized > 0.7,
            "at/above upper should be high, n={}",
            flexed.output_normalized
        );
    }

    #[test]
    fn output_never_goes_negative() {
        let mut ch = EmgChannelState::default();
        ch.lower_threshold = 20.0;
        ch.upper_threshold = 30.0;
        process_channel(&mut ch, &const_series(250, 1.0), 250.0);
        assert!(ch.output_normalized >= 0.0);
    }

    #[test]
    fn java_default_labels_match() {
        assert_eq!(EmgWindow::One.label(), "1.0 s");
        assert_eq!(EmgUvLimit::TwoHundred.label(), "200 uV");
        assert!((EmgChannelSettings::default().creep_increasing - 0.9).abs() < 1e-12);
        assert!((EmgChannelSettings::default().creep_decreasing - 0.99999).abs() < 1e-12);
    }

    #[test]
    fn unit_circle_map_matches_java_order() {
        let (x, y) = map_to_unit_circle(1.0, 1.0);
        // Java: x *= sqrt(1 - y^2/2) with y=1 → sqrt(0.5); then y *= sqrt(1 - x'^2/2)
        let x_java = (0.5_f64).sqrt();
        let y_java = 1.0 * (1.0 - (x_java * x_java) / 2.0).sqrt();
        assert!((x - x_java).abs() < 1e-12);
        assert!((y - y_java).abs() < 1e-12);
    }
}
