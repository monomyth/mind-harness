//! Labeled feature export (decoder prep). Windows of band power + marker + artifact.
//!
//! Schema (CSV header / JSONL keys):
//! `t0,t1,ch,delta,theta,alpha,beta,gamma,marker,artifact`
//!
//! No model training. `marker` is only what the operator typed.

use crate::board::BoardError;
use crate::fft::{band_powers_psd, nfft_safe};
use crate::markers::MarkerEvent;
use std::io::Write;
use std::path::Path;

/// One epoch × channel.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FeatureRow {
    pub t0: f64,
    pub t1: f64,
    pub ch: usize,
    pub delta: f64,
    pub theta: f64,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub marker: String,
    pub artifact: bool,
}

/// Java-ish 1 s window, 0.5 s hop. Artifact if population std > 100 µV.
pub const WINDOW_SEC: f64 = 1.0;
pub const HOP_SEC: f64 = 0.5;
pub const ARTIFACT_STD_UV: f64 = 100.0;

fn population_std(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    var.sqrt()
}

fn marker_in_window(markers: &[MarkerEvent], t0: f64, t1: f64, fs: f64) -> String {
    let mut labels = Vec::new();
    for m in markers {
        let t = if m.board_timestamp.is_finite() && m.board_timestamp >= 0.0 {
            m.board_timestamp
        } else {
            m.sample_index as f64 / fs.max(1.0)
        };
        if t >= t0 && t < t1 {
            labels.push(m.label.clone());
        }
    }
    labels.join("|")
}

/// `samples` is EXG-first: `samples[t][ch]`.
pub fn windowed_features(
    samples: &[Vec<f64>],
    sample_rate: i32,
    n_exg: usize,
    markers: &[MarkerEvent],
) -> Vec<FeatureRow> {
    if samples.is_empty() || n_exg == 0 {
        return vec![];
    }
    let fs = sample_rate.max(1) as f64;
    let win = ((WINDOW_SEC * fs) as usize).max(nfft_safe(sample_rate).min(samples.len()));
    let hop = ((HOP_SEC * fs) as usize).max(1);
    let n_ch = n_exg.min(samples[0].len());
    let mut rows = Vec::new();
    let mut start = 0usize;
    while start + win <= samples.len() {
        let t0 = start as f64 / fs;
        let t1 = (start + win) as f64 / fs;
        let label = marker_in_window(markers, t0, t1, fs);
        for ch in 0..n_ch {
            let col: Vec<f64> = samples
                .iter()
                .skip(start)
                .take(win)
                .map(|r| r.get(ch).copied().unwrap_or(0.0))
                .collect();
            let p = band_powers_psd(&col, fs);
            let artifact = population_std(&col) > ARTIFACT_STD_UV;
            rows.push(FeatureRow {
                t0,
                t1,
                ch,
                delta: p[0],
                theta: p[1],
                alpha: p[2],
                beta: p[3],
                gamma: p[4],
                marker: label.clone(),
                artifact,
            });
        }
        start += hop;
    }
    rows
}

pub fn write_csv(path: &Path, rows: &[FeatureRow]) -> Result<(), BoardError> {
    let mut f = std::fs::File::create(path).map_err(|e| BoardError::Io(e.to_string()))?;
    writeln!(f, "t0,t1,ch,delta,theta,alpha,beta,gamma,marker,artifact")
        .map_err(|e| BoardError::Io(e.to_string()))?;
    for r in rows {
        writeln!(
            f,
            "{:.6},{:.6},{},{:.8},{:.8},{:.8},{:.8},{:.8},{},{}",
            r.t0,
            r.t1,
            r.ch,
            r.delta,
            r.theta,
            r.alpha,
            r.beta,
            r.gamma,
            csv_escape(&r.marker),
            if r.artifact { "1" } else { "0" }
        )
        .map_err(|e| BoardError::Io(e.to_string()))?;
    }
    Ok(())
}

pub fn write_jsonl(path: &Path, rows: &[FeatureRow]) -> Result<(), BoardError> {
    let mut f = std::fs::File::create(path).map_err(|e| BoardError::Io(e.to_string()))?;
    for r in rows {
        let line = serde_json::to_string(r).map_err(|e| BoardError::Io(e.to_string()))?;
        writeln!(f, "{line}").map_err(|e| BoardError::Io(e.to_string()))?;
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Write CSV + JSONL next to a recording (`stem.features.csv` / `.jsonl`).
pub fn export_next_to(
    recording: &Path,
    samples: &[Vec<f64>],
    sample_rate: i32,
    n_exg: usize,
    markers: &[MarkerEvent],
) -> Result<(std::path::PathBuf, std::path::PathBuf), BoardError> {
    let rows = windowed_features(samples, sample_rate, n_exg, markers);
    let stem = recording.with_extension("");
    let csv = Path::new(&format!("{}.features.csv", stem.display())).to_path_buf();
    let jsonl = Path::new(&format!("{}.features.jsonl", stem.display())).to_path_buf();
    write_csv(&csv, &rows)?;
    write_jsonl(&jsonl, &rows)?;
    Ok((csv, jsonl))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markers::MarkerEvent;

    fn sine(n: usize, fs: f64, hz: f64) -> Vec<Vec<f64>> {
        (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                vec![(2.0 * std::f64::consts::PI * hz * t).sin() * 10.0]
            })
            .collect()
    }

    #[test]
    fn schema_row_has_ten_fields() {
        let samples = sine(500, 250.0, 10.0);
        let marks = vec![MarkerEvent {
            sample_index: 250,
            board_timestamp: 1.0,
            label: "blink".into(),
        }];
        let rows = windowed_features(&samples, 250, 1, &marks);
        assert!(!rows.is_empty());
        let hit = rows.iter().find(|r| r.marker.contains("blink"));
        assert!(hit.is_some(), "marker must land in a window");
        assert_eq!(hit.unwrap().ch, 0);
    }

    #[test]
    fn csv_header_matches_plan() {
        let dir = std::env::temp_dir();
        let path = dir.join("openbci_feat_test.csv");
        write_csv(&path, &[]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            body.trim(),
            "t0,t1,ch,delta,theta,alpha,beta,gamma,marker,artifact"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn high_amplitude_sets_artifact() {
        let samples: Vec<Vec<f64>> = (0..250)
            .map(|i| vec![if i % 2 == 0 { 0.0 } else { 400.0 }])
            .collect();
        let rows = windowed_features(&samples, 250, 1, &[]);
        assert!(rows.iter().any(|r| r.artifact));
    }
}
