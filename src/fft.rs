//! Simple FFT helper for the GUI.
//!
//! Uses `rustfft` under the hood. Returns frequency bins + magnitude in dB
//! for the first `max_freq` Hz (typical EEG interest is 0–100 Hz).

use rustfft::{num_complex::Complex, FftPlanner};
use std::f64::consts::PI;

/// Matches Java `W_FFT.pde` `fft_plot.setXLim(0.1, xLim)` so the DC bin is not drawn.
pub const FFT_DISPLAY_MIN_HZ: f64 = 0.1;

/// Java `getNfftSafe()` in OpenBCI_GUI.pde.
pub fn nfft_safe(sample_rate: i32) -> usize {
    match sample_rate {
        500 => 512,
        1000 => 1024,
        1600 => 2048,
        _ => 256,
    }
}

/// Compute a single-channel FFT magnitude spectrum.
/// `samples` should be a power-of-2 length for best performance (we zero-pad if needed).
///
/// Returns (frequencies, magnitudes_in_db) limited to `max_freq_hz`.
pub fn compute_fft_magnitude(
    samples: &[f64],
    sample_rate: f64,
    max_freq_hz: f64,
) -> (Vec<f64>, Vec<f64>) {
    if samples.is_empty() {
        return (vec![], vec![]);
    }

    let n = samples.len().next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);

    // Java DataProcessing: `fooData[I] -= meanData` before the Hamming FFT.
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let mut buffer: Vec<Complex<f64>> = samples
        .iter()
        .map(|&x| Complex {
            re: x - mean,
            im: 0.0,
        })
        .collect();
    buffer.resize(n, Complex { re: 0.0, im: 0.0 });

    // Java minim FFT uses HAMMING (`initializeFFTObjects` in DataProcessing.pde).
    let denom = (samples.len().saturating_sub(1)).max(1) as f64;
    for (i, val) in buffer.iter_mut().enumerate().take(samples.len()) {
        let w = 0.54 - 0.46 * (2.0 * PI * i as f64 / denom).cos();
        val.re *= w;
    }

    fft.process(&mut buffer);

    // Compute magnitudes in dB (20*log10)
    let mut magnitudes = Vec::with_capacity(n / 2);
    let mut frequencies = Vec::with_capacity(n / 2);

    let bin_width = sample_rate / n as f64;

    for (i, &val) in buffer.iter().enumerate().take(n / 2) {
        let freq = i as f64 * bin_width;
        // Hide residual DC / near-DC like the Java plot (xmin = 0.1 Hz).
        if freq < FFT_DISPLAY_MIN_HZ {
            continue;
        }
        if freq > max_freq_hz {
            break;
        }

        let mag = (val.re.powi(2) + val.im.powi(2)).sqrt();
        // Convert to dB, with a floor to avoid -inf
        let db = 20.0 * (mag / n as f64).max(1e-12).log10();
        frequencies.push(freq);
        magnitudes.push(db);
    }

    (frequencies, magnitudes)
}

/// Magnitude of the bin closest to `hz`, or `-inf` if the spectrum is empty.
pub fn bin_near(freqs: &[f64], mags: &[f64], hz: f64) -> f64 {
    freqs
        .iter()
        .zip(mags.iter())
        .min_by(|a, b| {
            (a.0 - hz)
                .abs()
                .partial_cmp(&(b.0 - hz).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, m)| *m)
        .unwrap_or(f64::NEG_INFINITY)
}

/// Line-noise peak the current notch does not cover (Java default is 50+60).
/// `mags` are linear µV; 4× vs the 10 Hz bin is 12 dB.
pub fn unmatched_mains_hz(
    freqs: &[f64],
    mags: &[f64],
    notch: crate::filter_settings::NotchMode,
) -> Option<f64> {
    use crate::filter_settings::NotchMode;
    if freqs.is_empty() {
        return None;
    }
    let at_10 = bin_near(freqs, mags, 10.0).max(1e-12);
    let at_50 = bin_near(freqs, mags, 50.0);
    let at_60 = bin_near(freqs, mags, 60.0);
    let covers_50 = matches!(notch, NotchMode::Fifty | NotchMode::FiftyAndSixty);
    let covers_60 = matches!(notch, NotchMode::Sixty | NotchMode::FiftyAndSixty);
    if !covers_50 && at_50 > at_10 * 4.0 && at_50 >= at_60 {
        Some(50.0)
    } else if !covers_60 && at_60 > at_10 * 4.0 && at_60 > at_50 {
        Some(60.0)
    } else {
        None
    }
}

/// Java `DataProcessing.processing_band_low_Hz` / `processing_band_high_Hz`.
pub const EEG_BANDS: &[(&str, f64, f64)] = &[
    ("Delta", 1.0, 4.0),
    ("Theta", 4.0, 8.0),
    ("Alpha", 8.0, 13.0),
    ("Beta", 13.0, 30.0),
    ("Gamma", 30.0, 55.0),
];

/// Histogram labels from Java `W_BandPower.update` (display only; computation uses `EEG_BANDS`).
pub const BAND_PLOT_LABELS: &[&str] = &[
    "DELTA\n0.5-4Hz",
    "THETA\n4-8Hz",
    "ALPHA\n8-13Hz",
    "BETA\n13-32Hz",
    "GAMMA\n32-100Hz",
];

/// Java minim single-sided amplitude (µV): |X|/N, interior bins × 2. Demean + Hamming.
pub fn fft_single_sided_uv(samples: &[f64], sample_rate: f64) -> (Vec<f64>, Vec<f64>) {
    if samples.is_empty() {
        return (vec![], vec![]);
    }
    let n = samples.len().next_power_of_two();
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let mut buffer: Vec<Complex<f64>> = samples
        .iter()
        .map(|&x| Complex {
            re: x - mean,
            im: 0.0,
        })
        .collect();
    buffer.resize(n, Complex { re: 0.0, im: 0.0 });
    let denom = (samples.len().saturating_sub(1)).max(1) as f64;
    for (i, val) in buffer.iter_mut().enumerate().take(samples.len()) {
        let w = 0.54 - 0.46 * (2.0 * PI * i as f64 / denom).cos();
        val.re *= w;
    }
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_forward(n).process(&mut buffer);

    let n_half = n / 2;
    let bin = sample_rate / n as f64;
    let mut freqs = Vec::with_capacity(n_half + 1);
    let mut mags = Vec::with_capacity(n_half + 1);
    for (k, val) in buffer.iter().enumerate().take(n_half + 1) {
        let raw = (val.re.powi(2) + val.im.powi(2)).sqrt();
        let mag = if k == 0 || k == n_half {
            raw / n as f64
        } else {
            2.0 * raw / n as f64
        };
        freqs.push(k as f64 * bin);
        mags.push(mag);
    }
    (freqs, mags)
}

/// Java W_FFT points: single-sided µV, xmin 0.1 Hz, xmax from the Max Freq dropdown.
pub fn fft_display_uv(samples: &[f64], sample_rate: f64, max_freq_hz: f64) -> (Vec<f64>, Vec<f64>) {
    let (freqs, mags) = fft_single_sided_uv(samples, sample_rate);
    freqs
        .into_iter()
        .zip(mags)
        .filter(|(f, _)| *f >= FFT_DISPLAY_MIN_HZ && *f <= max_freq_hz)
        .unzip()
}

/// Java `avgPowerInBins` one-bin PSD (µV)²/Hz.
fn psdx_at(i: usize, n_half: usize, mag: f64, n_f: f64, sample_rate: f64) -> f64 {
    if i != 0 && i != n_half {
        mag * mag * n_f / sample_rate / 4.0
    } else {
        mag * mag * n_f / sample_rate
    }
}

fn psd_sum_in_range(
    freqs: &[f64],
    mags: &[f64],
    n: usize,
    sample_rate: f64,
    low: f64,
    high: f64,
    exclude: Option<(f64, f64)>,
) -> f64 {
    if high <= low {
        return 0.0;
    }
    let n_half = freqs.len().saturating_sub(1);
    let n_f = n as f64;
    let mut sum = 0.0;
    for (i, (&f, &mag)) in freqs.iter().zip(mags.iter()).enumerate() {
        if f < low || f >= high {
            continue;
        }
        if let Some((elo, ehi)) = exclude {
            if f >= elo && f < ehi {
                continue;
            }
        }
        sum += psdx_at(i, n_half, mag, n_f, sample_rate);
    }
    sum
}

/// Sum of single-sided PSD in `[low, high)` Hz. Same formula as [`band_powers_psd`].
/// `exclude` skips `[lo, hi)` (notch hole at 60).
pub fn band_psd_excluding(
    samples: &[f64],
    sample_rate: f64,
    low: f64,
    high: f64,
    exclude: Option<(f64, f64)>,
) -> f64 {
    if samples.is_empty() || sample_rate <= 0.0 {
        return 0.0;
    }
    let n = samples.len().next_power_of_two();
    let (freqs, mags) = fft_single_sided_uv(samples, sample_rate);
    psd_sum_in_range(&freqs, &mags, n, sample_rate, low, high, exclude)
}

/// Java `avgPowerInBins`: sum of single-sided PSD in each band, (µV)²/Hz.
pub fn band_powers_psd(samples: &[f64], sample_rate: f64) -> [f64; 5] {
    if samples.is_empty() || sample_rate <= 0.0 {
        return [0.0; 5];
    }
    let n = samples.len().next_power_of_two();
    let (freqs, mags) = fft_single_sided_uv(samples, sample_rate);
    let mut out = [0.0; 5];
    for (b, &(_, low, high)) in EEG_BANDS.iter().enumerate() {
        out[b] = psd_sum_in_range(&freqs, &mags, n, sample_rate, low, high, None);
    }
    out
}

/// Java `updateBandPowerWidgetData`: mean of per-channel band PSD (not FFT of the mean waveform).
pub fn mean_band_powers(channels: &[Vec<f64>], sample_rate: f64) -> [f64; 5] {
    let mut acc = [0.0; 5];
    let mut n = 0usize;
    for ch in channels {
        if ch.is_empty() {
            continue;
        }
        let p = band_powers_psd(ch, sample_rate);
        for i in 0..5 {
            acc[i] += p[i];
        }
        n += 1;
    }
    if n > 0 {
        let nf = n as f64;
        for v in acc.iter_mut() {
            *v /= nf;
        }
    }
    acc
}

/// Named wrapper around [`band_powers_psd`] (Focus / table consumers).
pub fn compute_band_powers(samples: &[f64], sample_rate: f64) -> Vec<(String, f64)> {
    let p = band_powers_psd(samples, sample_rate);
    EEG_BANDS
        .iter()
        .zip(p.iter())
        .map(|((name, _, _), v)| (name.to_string(), *v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfft_matches_java_get_nfft_safe() {
        assert_eq!(nfft_safe(250), 256);
        assert_eq!(nfft_safe(125), 256);
        assert_eq!(nfft_safe(500), 512);
        assert_eq!(nfft_safe(1000), 1024);
    }

    #[test]
    fn empty_input_returns_empty_spectrum() {
        let (f, m) = compute_fft_magnitude(&[], 250.0, 100.0);
        assert!(f.is_empty());
        assert!(m.is_empty());
    }

    #[test]
    fn single_sample_does_not_panic() {
        let (f, m) = compute_fft_magnitude(&[1.0], 250.0, 100.0);
        assert_eq!(f.len(), m.len());
    }

    #[test]
    fn sine_at_10hz_has_peak_near_10hz() {
        let sr = 256.0;
        let n = 256;
        let samples: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 10.0 * i as f64 / sr).sin())
            .collect();
        let (freqs, mags) = compute_fft_magnitude(&samples, sr, 40.0);
        let (peak_f, _) = freqs
            .iter()
            .zip(mags.iter())
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!((*peak_f - 10.0).abs() < 2.0, "peak was {} Hz", peak_f);
    }

    #[test]
    fn spectrum_starts_at_java_xmin_not_dc() {
        let samples: Vec<f64> = (0..256).map(|i| 50.0 + (i as f64 * 0.01).sin()).collect();
        let (freqs, _) = compute_fft_magnitude(&samples, 250.0, 60.0);
        assert!(!freqs.is_empty());
        assert!(
            freqs.iter().all(|&f| f >= FFT_DISPLAY_MIN_HZ),
            "first bin was {} Hz",
            freqs[0]
        );
        assert!(!freqs.contains(&0.0));
    }

    #[test]
    fn demean_keeps_a_ten_hz_sine_above_a_dc_offset() {
        let sr = 256.0;
        let n = 256;
        let samples: Vec<f64> = (0..n)
            .map(|i| 200.0 + (2.0 * PI * 10.0 * i as f64 / sr).sin())
            .collect();
        let (freqs, mags) = compute_fft_magnitude(&samples, sr, 40.0);
        let (peak_f, _) = freqs
            .iter()
            .zip(mags.iter())
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!(
            (*peak_f - 10.0).abs() < 2.0,
            "Java demeans the FFT window so 200 µV DC cannot beat 10 Hz, peak={} Hz",
            peak_f
        );
    }

    #[test]
    fn unmatched_mains_flags_fifty_when_notch_is_sixty() {
        let freqs = [10.0, 50.0, 60.0];
        let mags = [1.0, 20.0, 0.5];
        assert_eq!(
            unmatched_mains_hz(&freqs, &mags, crate::filter_settings::NotchMode::Sixty),
            Some(50.0)
        );
        assert_eq!(
            unmatched_mains_hz(
                &freqs,
                &mags,
                crate::filter_settings::NotchMode::FiftyAndSixty
            ),
            None
        );
    }

    fn peak_hz(freqs: &[f64], mags: &[f64]) -> f64 {
        freqs
            .iter()
            .zip(mags.iter())
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(f, _)| *f)
            .unwrap()
    }

    #[test]
    fn sixty_hz_sine_peaks_near_sixty_not_fifty() {
        let sr = 250.0;
        let n = 256;
        let samples: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 60.0 * i as f64 / sr).sin())
            .collect();
        let (freqs, mags) = fft_display_uv(&samples, sr, 100.0);
        let peak = peak_hz(&freqs, &mags);
        assert!(
            (peak - 60.0).abs() < 2.0,
            "60 Hz must sit at 60 Hz on the FFT axis, peak={peak}"
        );
        assert!(
            bin_near(&freqs, &mags, 60.0) > 4.0 * bin_near(&freqs, &mags, 50.0).max(1e-12),
            "a 60 Hz tone must not look like a 50 Hz mountain"
        );
    }

    #[test]
    fn band_powers_have_five_standard_bands() {
        let names: Vec<_> = compute_band_powers(&[0.0; 64], 250.0)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["Delta", "Theta", "Alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn ten_hz_sine_is_strongest_in_alpha() {
        let sr = 250.0;
        let n = 256;
        let samples: Vec<f64> = (0..n)
            .map(|i| 10.0 * (2.0 * PI * 10.0 * i as f64 / sr).sin())
            .collect();
        let p = band_powers_psd(&samples, sr);
        let alpha = p[2];
        assert!(
            alpha > p[0] && alpha > p[1] && alpha > p[3] && alpha > p[4],
            "alpha should dominate a 10 Hz sine, bands={:?}",
            p
        );
        assert!(
            alpha > 1.0 && alpha < 40.0,
            "10 µV 10 Hz sine should land near ~10 on Java's 0.1–100 axis, not clip, alpha={}",
            alpha
        );
    }

    #[test]
    fn band_power_sums_psd_so_wider_bands_win_on_whiteish_noise() {
        let n = 256;
        let sr = 250.0;
        let samples: Vec<f64> = (0..n)
            .map(|i| ((i * 17) % 23) as f64 / 23.0 - 0.5)
            .collect();
        let p = band_powers_psd(&samples, sr);
        assert!(
            p[3] > p[1],
            "Java sums PSD in-band; beta (13–30) should beat theta (4–8) on broadband, beta={}, theta={}",
            p[3],
            p[1]
        );
    }

    #[test]
    fn average_band_powers_not_fft_of_averaged_waveforms() {
        let sr = 250.0;
        let n = 256;
        let a: Vec<f64> = (0..n)
            .map(|i| 10.0 * (2.0 * PI * 10.0 * i as f64 / sr).sin())
            .collect();
        let b: Vec<f64> = a.iter().map(|x| -x).collect();
        let mixed = mean_band_powers(&[a.clone(), b.clone()], sr);
        let mut avg_wave = vec![0.0; n];
        for i in 0..n {
            avg_wave[i] = (a[i] + b[i]) * 0.5;
        }
        let fft_of_mean = band_powers_psd(&avg_wave, sr);
        assert!(
            mixed[2] > 10.0 * fft_of_mean[2] + 1e-9,
            "Java averages per-channel PSD; averaging waveforms first cancels opposite-phase alpha, mixed_alpha={}, fft_mean_alpha={}",
            mixed[2],
            fft_of_mean[2]
        );
    }
}
