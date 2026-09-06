//! Shared Synthetic session helper for GROK_PLAN proofs (tests only).

use crate::board::brainflow_board::BrainFlowBoard;
use crate::board::{extract_exg, recent_raw_rows, DataSource};
use crate::data_logger::{DataLogger, LogFormat};
use crate::markers::MarkerEvent;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Parallel proof recordings must not write the same Recordings/ filename.
static RECORD_LOCK: Mutex<()> = Mutex::new(());

/// 10 s at the usual Cyton/Synthetic rate.
pub const SESSION_SAMPLES: usize = 2500;
pub const MARKS: &[(u64, &str)] = &[(500, "alpha_start"), (1250, "blink"), (2000, "end_task")];

static SYNTH_10S: OnceLock<(Vec<Vec<f64>>, i32)> = OnceLock::new();

/// Pull 10 s of BrainFlow Synthetic EXG (real board path, wall-clock ~10 s on first call).
pub fn synthetic_10s_exg() -> &'static (Vec<Vec<f64>>, i32) {
    SYNTH_10S.get_or_init(|| {
        let mut board = BrainFlowBoard::synthetic(8);
        board
            .initialize()
            .expect("Synthetic initialize (BrainFlow)");
        board.start_streaming().expect("Synthetic start_stream");
        let sr = board.sample_rate();
        let exg = board.exg_channels().to_vec();
        let mut samples = Vec::with_capacity(SESSION_SAMPLES);
        let deadline = Instant::now() + Duration::from_secs(25);
        while samples.len() < SESSION_SAMPLES {
            assert!(
                Instant::now() < deadline,
                "Synthetic produced {} / {SESSION_SAMPLES} samples in 25 s",
                samples.len()
            );
            board.update();
            for row in recent_raw_rows(&board) {
                samples.push(extract_exg(&row, &exg));
                if samples.len() >= SESSION_SAMPLES {
                    break;
                }
            }
            if samples.len() < SESSION_SAMPLES {
                std::thread::sleep(Duration::from_millis(4));
            }
        }
        let _ = board.stop_streaming();
        let _ = board.uninitialize();
        (samples, sr)
    })
}

pub fn record_marked_session(format: LogFormat) -> PathBuf {
    let _guard = RECORD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (samples, sr) = synthetic_10s_exg();
    let n_ch = samples[0].len();
    let mut logger = DataLogger::new();
    let path = logger.start(format, n_ch, *sr).expect("start recording");
    let mut mark_i = 0;
    for (i, row) in samples.iter().enumerate() {
        logger.log_sample(row, 0.0);
        let logged = (i + 1) as u64;
        if mark_i < MARKS.len() && logged == MARKS[mark_i].0 {
            logger
                .write_marker_annotation(0.0, MARKS[mark_i].1)
                .expect("mark");
            mark_i += 1;
        }
    }
    assert_eq!(mark_i, MARKS.len(), "all marks must be written");
    logger.stop();
    path
}

pub fn assert_marks_within_one_sample(got: &[MarkerEvent]) {
    for &(want_idx, label) in MARKS {
        let m = got
            .iter()
            .find(|m| m.label == label)
            .unwrap_or_else(|| panic!("missing mark {label} in {got:?}"));
        assert!(
            m.sample_index.abs_diff(want_idx) <= 1,
            "{label}: sample_index={} intended={want_idx}",
            m.sample_index
        );
        let fs = 250.0;
        let want_t = want_idx as f64 / fs;
        assert!(
            (m.board_timestamp - want_t).abs() <= 1.0 / fs + 1e-6,
            "{label}: board_timestamp={} intended={want_t}",
            m.board_timestamp
        );
    }
}

#[cfg(test)]
mod item3_markers {
    use super::*;
    use crate::board::playback::PlaybackBoard;
    use crate::board::DataSource;
    use crate::data_logger::LogFormat;
    use crate::markers;
    use crate::widgets::time_series::marker_visible_in_window;
    use std::io::Read;

    fn odf_file_marks(path: &std::path::Path) -> Vec<MarkerEvent> {
        let mut body = String::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        body.lines()
            .filter_map(markers::parse_odf_marker_line)
            .collect()
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(markers::sidecar_path(path));
    }

    #[test]
    fn ten_second_synthetic_odf_three_named_marks_and_playback_draws() {
        let path = record_marked_session(LogFormat::ODF);
        let odf_marks = odf_file_marks(&path);
        let sidecar = markers::load_sidecar(&path);
        assert_marks_within_one_sample(&odf_marks);
        assert_marks_within_one_sample(&sidecar);

        let mut pb = PlaybackBoard::from_file(&path).expect("playback ODF");
        assert_marks_within_one_sample(pb.session_markers());
        pb.seek_to_fraction(1.0);
        let playhead = pb.playhead_sample().unwrap();
        let visible = SESSION_SAMPLES;
        for &(idx, label) in MARKS {
            assert!(
                marker_visible_in_window(idx, playhead, visible),
                "Time Series window at end of 10 s file must include {label} (#{idx})"
            );
        }
        cleanup(&path);
    }

    #[test]
    fn ten_second_synthetic_bdf_three_named_marks() {
        let path = record_marked_session(LogFormat::BDF);
        let sidecar = markers::load_sidecar(&path);
        assert_marks_within_one_sample(&sidecar);
        let (_samples, _fs, _n, tal, _, _) = crate::data_writers::bdf::read_bdf(&path).expect("read bdf");
        assert_marks_within_one_sample(&tal);
        let pb = PlaybackBoard::from_file(&path).expect("playback BDF");
        assert_marks_within_one_sample(pb.session_markers());
        cleanup(&path);
    }
}

#[cfg(test)]
mod item5_bdf_playback {
    use super::*;
    use crate::board::playback::PlaybackBoard;
    use crate::board::DataSource;
    use crate::data_logger::LogFormat;
    use crate::markers;
    use crate::widgets::time_series::marker_visible_in_window;

    #[test]
    fn synthetic_bdf_playback_timeseries_and_marks_match_session() {
        let (orig, sr) = synthetic_10s_exg();
        let path = record_marked_session(LogFormat::BDF);
        let mut pb = PlaybackBoard::from_file(&path).expect("playback BDF");
        assert_eq!(pb.sample_rate(), *sr);
        assert_eq!(pb.exg_channels().len(), orig[0].len());
        assert_marks_within_one_sample(pb.session_markers());

        pb.seek_to_fraction(1.0);
        let raw = pb.get_raw_data(SESSION_SAMPLES);
        assert_eq!(raw.len(), SESSION_SAMPLES, "playback raw length");
        let n_ch = orig[0].len();
        let mut max_err = 0.0_f64;
        for (i, row) in raw.iter().enumerate() {
            for (ch, &a) in orig[i].iter().enumerate().take(n_ch) {
                let b = row.get(ch).copied().unwrap_or(0.0);
                max_err = max_err.max((a - b).abs());
            }
        }
        assert!(
            max_err < 2.0,
            "BDF playback EXG must match the Synthetic session, max_err={max_err} µV"
        );

        let playhead = pb.playhead_sample().unwrap();
        for &(idx, label) in MARKS {
            assert!(
                marker_visible_in_window(idx, playhead, SESSION_SAMPLES),
                "Time Series 10 s window must draw {label}"
            );
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(markers::sidecar_path(&path));
    }
}

#[cfg(test)]
mod item10_feature_export {
    use super::*;
    use crate::board::playback::PlaybackBoard;
    use crate::board::DataSource;
    use crate::data_logger::LogFormat;
    use crate::export;
    use crate::markers;

    #[test]
    fn marked_synthetic_session_exports_plan_schema() {
        let path = record_marked_session(LogFormat::ODF);
        let pb = PlaybackBoard::from_file(&path).expect("playback for export");
        let (csv, jsonl) = export::export_next_to(
            &path,
            pb.export_samples(),
            pb.sample_rate(),
            pb.exg_channels().len(),
            pb.session_markers(),
        )
        .expect("export");
        let csv_body = std::fs::read_to_string(&csv).unwrap();
        let header = csv_body.lines().next().unwrap();
        assert_eq!(
            header, "t0,t1,ch,delta,theta,alpha,beta,gamma,marker,artifact",
            "CSV schema must match GROK_PLAN item 10"
        );
        assert!(
            csv_body.contains("alpha_start")
                && csv_body.contains("blink")
                && csv_body.contains("end_task"),
            "export rows must carry the operator-typed marker labels"
        );
        let jsonl_body = std::fs::read_to_string(&jsonl).unwrap();
        let first: serde_json::Value =
            serde_json::from_str(jsonl_body.lines().next().unwrap()).unwrap();
        for key in [
            "t0", "t1", "ch", "delta", "theta", "alpha", "beta", "gamma", "marker", "artifact",
        ] {
            assert!(first.get(key).is_some(), "JSONL missing {key}");
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(markers::sidecar_path(&path));
        let _ = std::fs::remove_file(&csv);
        let _ = std::fs::remove_file(&jsonl);
    }
}

#[cfg(test)]
mod item10_parquet {
    use super::*;
    use crate::board::playback::PlaybackBoard;
    use crate::board::DataSource;
    use crate::data_logger::LogFormat;
    use crate::export::{export_recording, ExportKind};
    use crate::markers;

    #[test]
    fn parquet_session_plays_and_exports_bdf_text() {
        let path = record_marked_session(LogFormat::Parquet);
        assert_eq!(
            path.extension().and_then(|s| s.to_str()),
            Some("parquet")
        );
        let sidecar = markers::load_sidecar(&path);
        assert_marks_within_one_sample(&sidecar);
        let pb = PlaybackBoard::from_file(&path).expect("playback parquet");
        assert_marks_within_one_sample(pb.session_markers());
        let (bdf, _) = export_recording(&path, ExportKind::Bdf).expect("export bdf");
        let (txt, _) = export_recording(&path, ExportKind::OpenBciText).expect("export txt");
        let pb_bdf = PlaybackBoard::from_file(&bdf).expect("bdf from parquet");
        assert_eq!(pb_bdf.exg_channels().len(), 8);
        let body = std::fs::read_to_string(&txt).unwrap();
        assert!(body.contains("Sample Index, EXG Channel 0"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(markers::sidecar_path(&path));
        let _ = std::fs::remove_file(&bdf);
        let _ = std::fs::remove_file(&txt);
    }
}
