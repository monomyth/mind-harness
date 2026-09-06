//! Session export: Parquet → BDF / original OpenBCI text, plus labeled feature CSV/JSONL.

use crate::board::playback::PlaybackBoard;
use crate::board::{BoardError, DataSource};
use crate::data_logger::RecordingSample;
use crate::data_writers::bdf::{recording_signals_ex, DataWriterBDF};
use crate::fft::{band_powers_psd, nfft_safe};
use crate::markers::MarkerEvent;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What the top-bar Export control writes next to the take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Bdf,
    OpenBciText,
    Features,
}

impl ExportKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bdf => "BDF",
            Self::OpenBciText => "OpenBCI text",
            Self::Features => "Features",
        }
    }
}

fn sibling_with_ext(src: &Path, ext: &str) -> PathBuf {
    let cur = src
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if cur == ext {
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "OpenBCI".into());
        src.with_file_name(format!("{stem}_export.{ext}"))
    } else {
        src.with_extension(ext)
    }
}

fn playback_to_samples(pb: &PlaybackBoard) -> (Vec<RecordingSample>, usize, usize, usize, i32) {
    let n_exg = pb.exg_channels().len();
    let n_analog = pb.analog_channels().len();
    let n_digital = pb.digital_channels().len();
    let samples: Vec<RecordingSample> = pb
        .export_samples()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            RecordingSample::from_playback_row(row, n_exg, n_analog, n_digital)
                .with_time(i as f64 / pb.sample_rate().max(1) as f64)
        })
        .collect();
    (samples, n_exg, n_analog, n_digital, pb.sample_rate())
}

/// Convert any playable recording (Parquet / BDF / OpenBCI text) to `kind`.
pub fn export_recording(
    src: &Path,
    kind: ExportKind,
) -> Result<(PathBuf, Option<PathBuf>), BoardError> {
    match kind {
        ExportKind::Features => {
            let pb = PlaybackBoard::from_file(src).map_err(|e| BoardError::Io(e.to_string()))?;
            let (csv, jsonl) = export_next_to(
                src,
                pb.export_samples(),
                pb.sample_rate(),
                pb.exg_channels().len(),
                pb.session_markers(),
            )?;
            Ok((csv, Some(jsonl)))
        }
        ExportKind::Bdf => {
            let dest = sibling_with_ext(src, "bdf");
            convert_to_bdf(src, &dest)?;
            Ok((dest, None))
        }
        ExportKind::OpenBciText => {
            let dest = sibling_with_ext(src, "txt");
            convert_to_odf(src, &dest)?;
            Ok((dest, None))
        }
    }
}

pub fn convert_to_odf(src: &Path, dest: &Path) -> Result<(), BoardError> {
    let pb = PlaybackBoard::from_file(src).map_err(|e| BoardError::Io(e.to_string()))?;
    let (samples, n_exg, n_analog, n_digital, sr) = playback_to_samples(&pb);
    write_odf_file(dest, &samples, n_exg, n_analog, n_digital, sr, pb.session_markers())
        .map_err(|e| BoardError::Io(e.to_string()))
}

pub fn convert_to_bdf(src: &Path, dest: &Path) -> Result<(), BoardError> {
    let pb = PlaybackBoard::from_file(src).map_err(|e| BoardError::Io(e.to_string()))?;
    let (samples, n_exg, n_analog, n_digital, sr) = playback_to_samples(&pb);
    write_bdf_file(dest, &samples, n_exg, n_analog, n_digital, sr, pb.session_markers())
        .map_err(|e| BoardError::Io(e.to_string()))
}

fn write_odf_file(
    dest: &Path,
    samples: &[RecordingSample],
    n_exg: usize,
    n_analog: usize,
    n_digital: usize,
    sample_rate: i32,
    markers: &[MarkerEvent],
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(dest)?;
    writeln!(file, "%OpenBCI Raw EEG Data")?;
    writeln!(file, "%Number of channels = {n_exg}")?;
    writeln!(file, "%Sample Rate = {sample_rate} Hz")?;
    writeln!(file, "%First Column = SampleIndex")?;
    writeln!(file, "%Last 3 Columns = Accel Data (X, Y, Z)")?;
    let mut cols = vec!["Sample Index".to_string()];
    for i in 0..n_exg {
        cols.push(format!("EXG Channel {i}"));
    }
    for i in 0..n_analog {
        cols.push(format!("Analog Channel {i}"));
    }
    for i in 0..n_digital {
        cols.push(format!("Digital Channel {i}"));
    }
    cols.extend([
        "Accel Channel 0".into(),
        "Accel Channel 1".into(),
        "Accel Channel 2".into(),
    ]);
    writeln!(file, "{}", cols.join(", "))?;
    let mut mark_i = 0;
    let mut marks: Vec<&MarkerEvent> = markers.iter().collect();
    marks.sort_by_key(|m| m.sample_index);
    for (i, rec) in samples.iter().enumerate() {
        while mark_i < marks.len() && marks[mark_i].sample_index as usize == i {
            writeln!(file, "{}", marks[mark_i].odf_line())?;
            mark_i += 1;
        }
        let row = rec.odf_row_ex(n_exg, n_analog, n_digital);
        for (j, val) in row.iter().enumerate() {
            if j > 0 {
                write!(file, ", ")?;
            }
            if j == 0 {
                write!(file, "{:.0}", val)?;
            } else {
                write!(file, "{:.4}", val)?;
            }
        }
        writeln!(file)?;
    }
    while mark_i < marks.len() {
        writeln!(file, "{}", marks[mark_i].odf_line())?;
        mark_i += 1;
    }
    Ok(())
}

fn write_bdf_file(
    dest: &Path,
    samples: &[RecordingSample],
    n_exg: usize,
    n_analog: usize,
    n_digital: usize,
    sample_rate: i32,
    markers: &[MarkerEvent],
) -> std::io::Result<()> {
    let signals = recording_signals_ex(n_exg, n_analog, n_digital);
    let n_sig = signals.len();
    let mut bdf = DataWriterBDF::new(dest.to_path_buf(), signals, sample_rate)?;
    let spr = sample_rate.max(1) as usize;
    let mut buf = vec![Vec::new(); n_sig];
    let mut mark_i = 0;
    let mut marks: Vec<&MarkerEvent> = markers.iter().collect();
    marks.sort_by_key(|m| m.sample_index);
    for (i, rec) in samples.iter().enumerate() {
        while mark_i < marks.len() && marks[mark_i].sample_index as usize == i {
            let _ = bdf.write_annotation(marks[mark_i].board_timestamp, 0.0, &marks[mark_i].label);
            mark_i += 1;
        }
        let row = rec.bdf_row_ex(n_exg, n_analog, n_digital);
        for (ch, &v) in row.iter().enumerate() {
            if ch < buf.len() {
                buf[ch].push(v);
            }
        }
        if buf[0].len() >= spr {
            let record: Vec<Vec<f64>> = buf.iter().map(|ch| ch[..spr].to_vec()).collect();
            bdf.write_data_record(&record)?;
            for ch in &mut buf {
                *ch = ch[spr..].to_vec();
            }
        }
    }
    while mark_i < marks.len() {
        let _ = bdf.write_annotation(marks[mark_i].board_timestamp, 0.0, &marks[mark_i].label);
        mark_i += 1;
    }
    if !buf.is_empty() && !buf[0].is_empty() {
        bdf.write_data_record(&buf)?;
    }
    bdf.close()?;
    Ok(())
}

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

    #[test]
    fn parquet_export_emits_bdf_and_openbci_text() {
        use crate::data_logger::{DataLogger, LogFormat, RecordingSample};
        let mut logger = DataLogger::new();
        let path = logger.start(LogFormat::Parquet, 8, 250).unwrap();
        for i in 0..20 {
            logger.log_recording(&RecordingSample {
                packet_index: i as f64,
                exg: vec![i as f64; 8],
                accel: [0.1, -0.2, 0.9],
                time: i as f64 / 250.0,
                ..Default::default()
            });
        }
        logger.stop();
        let (bdf, _) = export_recording(&path, ExportKind::Bdf).expect("bdf");
        let (txt, _) = export_recording(&path, ExportKind::OpenBciText).expect("txt");
        assert!(bdf.extension().and_then(|s| s.to_str()) == Some("bdf"));
        assert!(txt.extension().and_then(|s| s.to_str()) == Some("txt"));
        let body = std::fs::read_to_string(&txt).unwrap();
        assert!(body.contains("%First Column = SampleIndex"));
        assert!(body.contains("%Last 3 Columns = Accel Data (X, Y, Z)"));
        let (samples, fs, n_exg, _, _, _) =
            crate::data_writers::bdf::read_bdf(&bdf).expect("read exported bdf");
        assert_eq!(fs, 250);
        assert_eq!(n_exg, 8);
        assert!(samples.len() >= 20);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bdf);
        let _ = std::fs::remove_file(&txt);
        let _ = std::fs::remove_file(crate::markers::sidecar_path(&path));
    }
}
