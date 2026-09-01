//! Pairwise slow-wave lag (Hilbert PLV + lagged cross-correlation).
//!
//! Not a traveling-wave field. Not circular. Not an origin or path in the skull.
//! Default Cyton 8ch (Mark IV docs): front is Fp1+Fp2, back is O1+O2.
//!
//! Zero lag (including |lag| ≤ 1 sample) is treated as volume conduction, not a direction.

use rustfft::{num_complex::Complex, FftPlanner};
use std::f64::consts::PI;

/// Named 10-20 order used by Head Plot (official Mark IV Cyton 8ch).
pub const MONTAGE: [&str; 8] = ["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"];
pub const IDX_FP1: usize = 0;
pub const IDX_FP2: usize = 1;
pub const IDX_C3: usize = 2;
pub const IDX_C4: usize = 3;
pub const IDX_P7: usize = 4;
pub const IDX_P8: usize = 5;
pub const IDX_O1: usize = 6;
pub const IDX_O2: usize = 7;

pub const WINDOW_SEC: f64 = 2.0;
/// |lag| at or below this many samples is volume conduction, not travel.
pub const ZERO_LAG_SAMPLES: i32 = 1;
const CONF_MIN: f64 = 0.55;
const INBAND_RMS_RATIO: f64 = 0.12;
const MIN_OVERLAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Default: 0.5–2 Hz.
    Slow,
    /// 4–8 Hz.
    Theta,
}

impl Band {
    pub fn hz(self) -> (f64, f64) {
        match self {
            Band::Slow => (0.5, 2.0),
            Band::Theta => (4.0, 8.0),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Band::Slow => "0.5–2 Hz",
            Band::Theta => "4–8 Hz",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pair {
    /// Fp1+Fp2 vs O1+O2.
    AnteriorPosterior,
    /// C3 vs C4.
    LeftRight,
    /// Signed Which first plate: O1 vs O2.
    O1O2,
}

impl Pair {
    pub fn label(self) -> &'static str {
        match self {
            Pair::AnteriorPosterior => "Fp vs O",
            Pair::LeftRight => "C3 vs C4",
            Pair::O1O2 => "O1 vs O2",
        }
    }

    /// Two insert holes named/filled on the Mark IV. A-P is Fp1 vs O1;
    /// L-R is C3 vs C4. Never the other six.
    pub fn draw_idx(self) -> [usize; 2] {
        match self {
            Pair::LeftRight => [IDX_C3, IDX_C4],
            Pair::AnteriorPosterior => [IDX_FP1, IDX_O1],
            Pair::O1O2 => [IDX_O1, IDX_O2],
        }
    }

    pub fn site_idx(self) -> [usize; 2] {
        self.draw_idx()
    }

    pub fn site_names(self) -> [&'static str; 2] {
        match self {
            Pair::LeftRight => ["C3", "C4"],
            Pair::AnteriorPosterior => ["Fp1", "O1"],
            Pair::O1O2 => ["O1", "O2"],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LagResult {
    pub band: Band,
    pub pair: Pair,
    pub lag_ms: f64,
    /// None when volume conduction or confidence is too low.
    pub direction: Option<String>,
    pub conf: f64,
}

impl LagResult {
    pub fn is_volume_conduction(&self) -> bool {
        self.direction.is_none() && self.conf >= CONF_MIN
    }

    pub fn human_copy(&self) -> String {
        if let Some(dir) = &self.direction {
            format!(
                "{} · {} · {} · conf {:.0}%",
                self.band.label(),
                self.pair.label(),
                dir,
                self.conf * 100.0
            )
        } else if self.is_volume_conduction() {
            format!(
                "{} · {} · volume conduction ({:.0} ms) · conf {:.0}%",
                self.band.label(),
                self.pair.label(),
                self.lag_ms.abs(),
                self.conf * 100.0
            )
        } else {
            format!(
                "{} · {} · low confidence · conf {:.0}%",
                self.band.label(),
                self.pair.label(),
                self.conf * 100.0
            )
        }
    }
}

/// Bandpass `channels` (logical 8ch order) and lag the two named pairs.
pub fn analyze_window(channels: &[Vec<f64>], sr: f64, band: Band) -> Vec<LagResult> {
    vec![
        analyze_ap(channels, sr, band),
        analyze_lr(channels, sr, band),
        analyze_o1o2(channels, sr, band),
    ]
}

fn analyze_ap(channels: &[Vec<f64>], sr: f64, band: Band) -> LagResult {
    let front = mean_channels(channels, &[IDX_FP1, IDX_FP2]);
    let back = mean_channels(channels, &[IDX_O1, IDX_O2]);
    pair_lag(band, Pair::AnteriorPosterior, "Fp", "O", &front, &back, sr)
}

fn analyze_lr(channels: &[Vec<f64>], sr: f64, band: Band) -> LagResult {
    let c3 = channels.get(IDX_C3).cloned().unwrap_or_default();
    let c4 = channels.get(IDX_C4).cloned().unwrap_or_default();
    pair_lag(band, Pair::LeftRight, "C3", "C4", &c3, &c4, sr)
}

fn analyze_o1o2(channels: &[Vec<f64>], sr: f64, band: Band) -> LagResult {
    let o1 = channels.get(IDX_O1).cloned().unwrap_or_default();
    let o2 = channels.get(IDX_O2).cloned().unwrap_or_default();
    pair_lag(band, Pair::O1O2, "O1", "O2", &o1, &o2, sr)
}

/// Lag of `b` relative to `a`. Positive lag_ms ⇒ `a` leads `b`.
fn pair_lag(
    band: Band,
    pair: Pair,
    name_a: &str,
    name_b: &str,
    a: &[f64],
    b: &[f64],
    sr: f64,
) -> LagResult {
    let empty = LagResult {
        band,
        pair,
        lag_ms: 0.0,
        direction: None,
        conf: 0.0,
    };
    if a.len() < MIN_OVERLAP || b.len() < MIN_OVERLAP || sr <= 1.0 {
        return empty;
    }
    let (lo, hi) = band.hz();
    let a_bp = fft_bandpass(a, sr, lo, hi);
    let b_bp = fft_bandpass(b, sr, lo, hi);
    let rms_a = rms(&a_bp);
    let rms_b = rms(&b_bp);
    let inband = (rms_a / rms(a).max(1e-12)).min(rms_b / rms(b).max(1e-12));
    if inband < INBAND_RMS_RATIO {
        return empty;
    }

    let max_lag = ((0.150 * sr).round() as i32).max(ZERO_LAG_SAMPLES + 1);
    let (lag_samples, xcorr) = xcorr_peak(&a_bp, &b_bp, max_lag);
    let plv = hilbert_plv(&a_bp, &b_bp);
    let conf = (xcorr.max(0.0) * plv * inband.clamp(0.0, 1.0))
        .sqrt()
        .clamp(0.0, 1.0);
    let lag_ms = lag_samples as f64 / sr * 1000.0;

    let direction = if conf < CONF_MIN || lag_samples.abs() <= ZERO_LAG_SAMPLES {
        None
    } else if lag_samples > 0 {
        Some(format!("{name_a} leads {name_b} by {:.0} ms", lag_ms.abs()))
    } else {
        Some(format!("{name_b} leads {name_a} by {:.0} ms", lag_ms.abs()))
    };

    LagResult {
        band,
        pair,
        lag_ms,
        direction,
        conf,
    }
}

/// Lag of `b` relative to `a`. Positive lag_ms => `a` leads `b`.
pub fn lag_named(
    band: Band,
    name_a: &str,
    name_b: &str,
    a: &[f64],
    b: &[f64],
    sr: f64,
) -> LagResult {
    pair_lag(band, Pair::LeftRight, name_a, name_b, a, b, sr)
}

fn mean_channels(channels: &[Vec<f64>], idxs: &[usize]) -> Vec<f64> {
    let n = idxs
        .iter()
        .filter_map(|&i| channels.get(i).map(|c| c.len()))
        .min()
        .unwrap_or(0);
    let mut out = vec![0.0; n];
    if idxs.is_empty() {
        return out;
    }
    for (t, v) in out.iter_mut().enumerate() {
        let mut s = 0.0;
        let mut c = 0.0;
        for &i in idxs {
            if let Some(ch) = channels.get(i) {
                if t < ch.len() {
                    s += ch[t];
                    c += 1.0;
                }
            }
        }
        *v = if c > 0.0 { s / c } else { 0.0 };
    }
    out
}

fn rms(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn fft_bandpass(x: &[f64], sr: f64, lo: f64, hi: f64) -> Vec<f64> {
    let n = x.len();
    if n < 16 || sr <= 0.0 {
        return vec![0.0; n];
    }
    let mean = x.iter().sum::<f64>() / n as f64;
    let mut buf: Vec<Complex<f64>> = x
        .iter()
        .map(|&v| Complex {
            re: v - mean,
            im: 0.0,
        })
        .collect();
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_forward(n).process(&mut buf);
    let df = sr / n as f64;
    for (k, bin) in buf.iter_mut().enumerate() {
        let f = if k <= n / 2 {
            k as f64 * df
        } else {
            (n - k) as f64 * df
        };
        if f < lo || f > hi {
            *bin = Complex::new(0.0, 0.0);
        }
    }
    planner.plan_fft_inverse(n).process(&mut buf);
    let scale = 1.0 / n as f64;
    buf.iter().map(|c| c.re * scale).collect()
}

/// Analytic signal via FFT Hilbert: zero negative frequencies, IFFT.
fn analytic(x: &[f64]) -> Vec<Complex<f64>> {
    let n = x.len();
    if n < 16 {
        return vec![Complex::new(0.0, 0.0); n];
    }
    let mut buf: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v, 0.0)).collect();
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_forward(n).process(&mut buf);
    // Keep DC; double 1..n/2-1; keep Nyquist if even; zero the upper half.
    for bin in buf.iter_mut().take(n).skip(n / 2 + 1) {
        *bin = Complex::new(0.0, 0.0);
    }
    if n > 2 {
        let last_pos = if n.is_multiple_of(2) {
            n / 2 - 1
        } else {
            n / 2
        };
        for bin in buf.iter_mut().take(last_pos + 1).skip(1) {
            *bin = Complex::new(bin.re * 2.0, bin.im * 2.0);
        }
    }
    planner.plan_fft_inverse(n).process(&mut buf);
    let scale = 1.0 / n as f64;
    buf.iter()
        .map(|c| Complex::new(c.re * scale, c.im * scale))
        .collect()
}

fn hilbert_plv(x: &[f64], y: &[f64]) -> f64 {
    let ax = analytic(x);
    let ay = analytic(y);
    let n = ax.len().min(ay.len());
    if n == 0 {
        return 0.0;
    }
    let mut re = 0.0;
    let mut im = 0.0;
    for i in 0..n {
        let px = ax[i].im.atan2(ax[i].re);
        let py = ay[i].im.atan2(ay[i].re);
        let d = py - px;
        re += d.cos();
        im += d.sin();
    }
    (re * re + im * im).sqrt() / n as f64
}

/// Peak Pearson lag. Positive lag ⇒ `y` is delayed relative to `x` (`x` leads).
fn xcorr_peak(x: &[f64], y: &[f64], max_lag: i32) -> (i32, f64) {
    let mut best_lag = 0i32;
    let mut best = f64::NEG_INFINITY;
    for lag in -max_lag..=max_lag {
        if let Some(r) = pearson_at_lag(x, y, lag) {
            if r > best {
                best = r;
                best_lag = lag;
            }
        }
    }
    if best.is_finite() {
        (best_lag, best)
    } else {
        (0, 0.0)
    }
}

fn pearson_at_lag(x: &[f64], y: &[f64], lag: i32) -> Option<f64> {
    let n = x.len().min(y.len()) as i32;
    let (x0, y0, len) = if lag >= 0 {
        (0, lag, n - lag)
    } else {
        (-lag, 0, n + lag)
    };
    if len < MIN_OVERLAP as i32 {
        return None;
    }
    let len = len as usize;
    let x0 = x0 as usize;
    let y0 = y0 as usize;
    let xs = &x[x0..x0 + len];
    let ys = &y[y0..y0 + len];
    let mx = xs.iter().sum::<f64>() / len as f64;
    let my = ys.iter().sum::<f64>() / len as f64;
    let mut num = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..len {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        num += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    let den = (vx * vy).sqrt();
    if den < 1e-18 {
        return None;
    }
    Some(num / den)
}

pub struct SelfTestReport {
    pub passed: usize,
    pub failed: usize,
    pub lines: Vec<String>,
}

impl SelfTestReport {
    pub fn ok(&self) -> bool {
        self.failed == 0 && self.passed > 0
    }
}

/// Same fixture vectors the unit tests use — for the PROPERTIES Self-test button.
pub fn run_self_test() -> SelfTestReport {
    let mut lines = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    for (name, ok, detail) in fixture_checks() {
        if ok {
            passed += 1;
            lines.push(format!("PASS  {name}  {detail}"));
        } else {
            failed += 1;
            lines.push(format!("FAIL  {name}  {detail}"));
        }
    }
    SelfTestReport {
        passed,
        failed,
        lines,
    }
}

fn fixture_checks() -> Vec<(&'static str, bool, String)> {
    let sr = 250.0;
    let n = 500usize;
    let delay_s = 0.040;
    let ap = eight_ch_ap_delay(n, sr, 1.0, delay_s);
    let lr = eight_ch_lr_delay(n, sr, 1.0, delay_s);
    let same = eight_ch_identical(n, sr, 1.0);

    let ap_slow = analyze_ap(&ap, sr, Band::Slow);
    let lr_slow = analyze_lr(&lr, sr, Band::Slow);
    let vc = analyze_ap(&same, sr, Band::Slow);
    let theta_on_1hz = analyze_ap(&ap, sr, Band::Theta);

    vec![
        (
            "Fp vs O delay",
            ap_slow.direction.as_deref() == Some("Fp leads O by 40 ms")
                && (ap_slow.lag_ms - 40.0).abs() <= 6.0,
            ap_slow.human_copy(),
        ),
        (
            "C3 vs C4 delay",
            lr_slow.direction.as_deref() == Some("C3 leads C4 by 40 ms")
                && (lr_slow.lag_ms - 40.0).abs() <= 6.0,
            lr_slow.human_copy(),
        ),
        (
            "zero lag",
            vc.direction.is_none() && vc.is_volume_conduction() && vc.lag_ms.abs() <= 6.0,
            vc.human_copy(),
        ),
        (
            "4–8 Hz on 1 Hz",
            theta_on_1hz.direction.is_none() && theta_on_1hz.conf < CONF_MIN,
            theta_on_1hz.human_copy(),
        ),
    ]
}

fn sine(n: usize, sr: f64, hz: f64, delay_s: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = i as f64 / sr - delay_s;
            (2.0 * PI * hz * t).cos()
        })
        .collect()
}

fn eight_ch_ap_delay(n: usize, sr: f64, hz: f64, delay_s: f64) -> Vec<Vec<f64>> {
    let front = sine(n, sr, hz, 0.0);
    let back = sine(n, sr, hz, delay_s);
    let mid = sine(n, sr, hz, delay_s * 0.5);
    vec![
        front.clone(),
        front,
        mid.clone(),
        mid.clone(),
        mid.clone(),
        mid,
        back.clone(),
        back,
    ]
}

fn eight_ch_lr_delay(n: usize, sr: f64, hz: f64, delay_s: f64) -> Vec<Vec<f64>> {
    let left = sine(n, sr, hz, 0.0);
    let right = sine(n, sr, hz, delay_s);
    let other = sine(n, sr, hz, 0.0);
    vec![
        other.clone(),
        other.clone(),
        left,
        right,
        other.clone(),
        other.clone(),
        other.clone(),
        other,
    ]
}

fn eight_ch_identical(n: usize, sr: f64, hz: f64) -> Vec<Vec<f64>> {
    let s = sine(n, sr, hz, 0.0);
    vec![s; 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_sine_fp_leads_p() {
        let sr = 250.0;
        let ch = eight_ch_ap_delay(500, sr, 1.0, 0.040);
        let r = analyze_ap(&ch, sr, Band::Slow);
        assert_eq!(r.pair, Pair::AnteriorPosterior);
        assert_eq!(r.band, Band::Slow);
        assert!(
            (r.lag_ms - 40.0).abs() <= 6.0,
            "lag_ms={} want ~40",
            r.lag_ms
        );
        assert_eq!(r.direction.as_deref(), Some("Fp leads O by 40 ms"));
        assert!(r.conf >= CONF_MIN, "conf={}", r.conf);
    }

    #[test]
    fn delayed_sine_p_leads_fp() {
        let sr = 250.0;
        // Negative delay on back = back happens first.
        let ch = eight_ch_ap_delay(500, sr, 1.0, -0.040);
        let r = analyze_ap(&ch, sr, Band::Slow);
        assert!(
            (r.lag_ms + 40.0).abs() <= 6.0,
            "lag_ms={} want ~-40",
            r.lag_ms
        );
        assert_eq!(r.direction.as_deref(), Some("O leads Fp by 40 ms"));
    }

    #[test]
    fn delayed_sine_c3_leads_c4() {
        let sr = 250.0;
        let ch = eight_ch_lr_delay(500, sr, 1.0, 0.040);
        let r = analyze_lr(&ch, sr, Band::Slow);
        assert_eq!(r.pair, Pair::LeftRight);
        assert!(
            (r.lag_ms - 40.0).abs() <= 6.0,
            "lag_ms={} want ~40",
            r.lag_ms
        );
        assert_eq!(r.direction.as_deref(), Some("C3 leads C4 by 40 ms"));
        assert!(r.conf >= CONF_MIN, "conf={}", r.conf);
    }

    #[test]
    fn delayed_sine_c4_leads_c3() {
        let sr = 250.0;
        let ch = eight_ch_lr_delay(500, sr, 1.0, -0.040);
        let r = analyze_lr(&ch, sr, Band::Slow);
        assert_eq!(r.direction.as_deref(), Some("C4 leads C3 by 40 ms"));
    }

    #[test]
    fn identical_sines_are_volume_conduction() {
        let sr = 250.0;
        let ch = eight_ch_identical(500, sr, 1.0);
        let ap = analyze_ap(&ch, sr, Band::Slow);
        let lr = analyze_lr(&ch, sr, Band::Slow);
        assert!(ap.direction.is_none(), "AP {:?}", ap.direction);
        assert!(lr.direction.is_none(), "LR {:?}", lr.direction);
        assert!(ap.is_volume_conduction(), "AP copy {}", ap.human_copy());
        assert!(lr.is_volume_conduction(), "LR copy {}", lr.human_copy());
        assert!(ap.lag_ms.abs() <= 6.0, "AP lag {}", ap.lag_ms);
        assert!(lr.lag_ms.abs() <= 6.0, "LR lag {}", lr.lag_ms);
    }

    #[test]
    fn theta_band_does_not_fire_on_one_hz_delay() {
        let sr = 250.0;
        let ch = eight_ch_ap_delay(500, sr, 1.0, 0.040);
        let r = analyze_ap(&ch, sr, Band::Theta);
        assert!(r.direction.is_none(), "dir={:?}", r.direction);
        assert!(
            r.conf < CONF_MIN,
            "1 Hz energy must not pass 4–8 Hz, conf={}",
            r.conf
        );
    }

    #[test]
    fn analyze_window_covers_both_pairs() {
        let sr = 250.0;
        let mut ch = eight_ch_ap_delay(500, sr, 1.0, 0.040);
        // Also delay C4 so LR is defined on the same window.
        ch[IDX_C4] = sine(500, sr, 1.0, 0.040);
        let out = analyze_window(&ch, sr, Band::Slow);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].pair, Pair::AnteriorPosterior);
        assert_eq!(out[1].pair, Pair::LeftRight);
        assert_eq!(out[2].pair, Pair::O1O2);
        assert!(out[0].direction.is_some());
        assert!(out[1].direction.is_some());
    }

    #[test]
    fn montage_is_mark_iv_cyton_8ch() {
        assert_eq!(MONTAGE, ["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"]);
        assert_eq!(MONTAGE[IDX_FP1], "Fp1");
        assert_eq!(MONTAGE[IDX_C3], "C3");
        assert_eq!(MONTAGE[IDX_O2], "O2");
    }

    #[test]
    fn self_test_fixtures_pass() {
        let report = run_self_test();
        assert!(
            report.ok(),
            "self-test failed:\n{}",
            report.lines.join("\n")
        );
    }
}
