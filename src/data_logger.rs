//! Data logging (ODF + BDF) — improved version.
//!
//! Supports both OpenBCI Data Format (text) and BDF+ (binary).

use crate::data_writers::bdf::DataWriterBDF;
use crate::markers::{self, MarkerEvent};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum LogFormat {
    ODF,
    BDF,
}

pub struct DataLogger {
    odf_writer: Option<Box<dyn Write + Send>>,
    bdf_writer: Option<DataWriterBDF>,
    rows_written: u64,
    output_path: Option<PathBuf>,
    format: LogFormat,
    recording_start: Option<std::time::Instant>,
    // For BDF we buffer per channel until we have enough for a data record
    bdf_buffer: Vec<Vec<f64>>,
    samples_per_record: usize,
    sample_rate: i32,
    samples_logged: u64,
    markers: Vec<MarkerEvent>,
}

/// Stable Recordings folder: crate Recordings/ if we can see Cargo.toml, else ~/Recordings.
/// Never the process cwd (launching the app from home used to dump files there).
pub fn recordings_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            if ancestor.join("Cargo.toml").is_file() {
                let d = ancestor.join("Recordings");
                let _ = std::fs::create_dir_all(&d);
                return d;
            }
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let d = home.join("Recordings");
    let _ = std::fs::create_dir_all(&d);
    d
}

static RECORDING_SEQ: AtomicU64 = AtomicU64::new(0);

impl DataLogger {
    pub fn new() -> Self {
        Self {
            odf_writer: None,
            bdf_writer: None,
            rows_written: 0,
            output_path: None,
            format: LogFormat::ODF,
            recording_start: None,
            bdf_buffer: vec![],
            samples_per_record: 0,
            sample_rate: 250,
            samples_logged: 0,
            markers: Vec::new(),
        }
    }

    pub fn start(
        &mut self,
        format: LogFormat,
        nb_channels: usize,
        sample_rate: i32,
    ) -> std::io::Result<PathBuf> {
        self.stop();

        let rec_dir = recordings_dir();

        // Generate timestamped filename so we never overwrite previous recordings
        let now = chrono::Local::now();
        let seq = RECORDING_SEQ.fetch_add(1, Ordering::Relaxed);
        let timestamp = format!(
            "{}_{}_{}",
            now.format("%Y-%m-%d_%H-%M-%S_%3f"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0),
            seq
        );

        let (path, _filename) = match format {
            LogFormat::ODF => {
                let filename = format!("OpenBCI_{}.txt", timestamp);
                let path = rec_dir.join(&filename);
                let mut file = File::create(&path)?;
                writeln!(file, "OpenBCI Data Format (Rust port) - {}", timestamp)?;
                writeln!(
                    file,
                    "Sample Rate: {} Hz, Channels: {}",
                    sample_rate, nb_channels
                )?;
                // Header names EXG columns only. Do not prefix a timestamp column — the
                // playback parser treats the first numeric field as ch0.
                let ch_header: String = (0..nb_channels)
                    .map(|i| format!("ch{}", i))
                    .collect::<Vec<_>>()
                    .join(",");
                writeln!(file, "{}", ch_header)?;
                self.odf_writer = Some(Box::new(file));
                self.output_path = Some(path.clone());
                (path, filename)
            }
            LogFormat::BDF => {
                let filename = format!("OpenBCI_{}.bdf", timestamp);
                let path = rec_dir.join(&filename);
                let bdf = DataWriterBDF::new(path.clone(), nb_channels, sample_rate)?;
                self.bdf_writer = Some(bdf);
                self.output_path = Some(path.clone());
                self.samples_per_record = sample_rate as usize;
                self.bdf_buffer = vec![Vec::new(); nb_channels];
                (path, filename)
            }
        };

        self.format = format;
        self.rows_written = 0;
        self.samples_logged = 0;
        self.markers.clear();
        self.sample_rate = sample_rate;
        self.recording_start = Some(std::time::Instant::now());

        // Experiment markers go to TAL / ODF; do not queue bookkeeping text that
        // would force an extra empty BDF record on close.

        Ok(path)
    }

    pub fn log_sample(&mut self, sample: &[f64], _timestamp: f64) {
        match self.format {
            LogFormat::ODF => {
                if let Some(ref mut w) = self.odf_writer {
                    for (i, val) in sample.iter().enumerate() {
                        if i > 0 {
                            let _ = write!(w, ",");
                        }
                        let _ = write!(w, "{:.4}", val);
                    }
                    let _ = writeln!(w);
                    self.rows_written += 1;
                    self.samples_logged += 1;
                }
            }
            LogFormat::BDF => {
                if self.bdf_buffer.is_empty() {
                    return;
                }

                // Accumulate per channel
                for (i, &val) in sample.iter().enumerate() {
                    if i < self.bdf_buffer.len() {
                        self.bdf_buffer[i].push(val);
                    }
                }
                self.samples_logged += 1;

                // When we have a full record, write it
                if !self.bdf_buffer.is_empty()
                    && self.bdf_buffer[0].len() >= self.samples_per_record
                {
                    let record: Vec<Vec<f64>> = self
                        .bdf_buffer
                        .iter()
                        .map(|ch| ch[..self.samples_per_record].to_vec())
                        .collect();

                    if let Some(ref mut bdf) = self.bdf_writer {
                        let _ = bdf.write_data_record(&record);
                        self.rows_written += 1;
                    }

                    // Remove the written samples
                    for ch in &mut self.bdf_buffer {
                        *ch = ch[self.samples_per_record..].to_vec();
                    }
                }
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut bdf) = self.bdf_writer.take() {
            // Flush any remaining samples
            if !self.bdf_buffer.is_empty() && !self.bdf_buffer[0].is_empty() {
                let _ = bdf.write_data_record(&self.bdf_buffer);
            }
            let _ = bdf.close();
        }
        self.odf_writer = None;
        self.output_path = None;
        self.rows_written = 0;
        self.samples_logged = 0;
        self.bdf_buffer.clear();
        self.samples_per_record = 0;
        self.recording_start = None;
    }

    pub fn is_logging(&self) -> bool {
        self.odf_writer.is_some() || self.bdf_writer.is_some()
    }

    pub fn recording_duration(&self) -> Option<std::time::Duration> {
        self.recording_start.map(|t| t.elapsed())
    }

    pub fn current_file(&self) -> Option<&PathBuf> {
        self.output_path.as_ref()
    }

    #[allow(dead_code)]
    pub fn samples_logged(&self) -> u64 {
        self.samples_logged
    }

    #[allow(dead_code)]
    pub fn markers(&self) -> &[MarkerEvent] {
        &self.markers
    }

    pub fn board_time(&self) -> f64 {
        self.samples_logged as f64 / (self.sample_rate.max(1) as f64)
    }

    /// Sample-accurate mark: index = samples already written, time = index / fs.
    pub fn write_marker_annotation(&mut self, _onset_unix: f64, text: &str) -> std::io::Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        let sample_index = self.samples_logged;
        let board_timestamp = self.board_time();
        let event = MarkerEvent::new(sample_index, board_timestamp, text.trim());
        if let Some(path) = self.output_path.clone() {
            let _ = markers::append_sidecar(&path, &event);
        }
        if let Some(ref mut bdf) = self.bdf_writer {
            bdf.write_annotation(board_timestamp, 0.0, &event.label)?;
        } else if let Some(ref mut w) = self.odf_writer {
            writeln!(w, "{}", event.odf_line())?;
        }
        self.markers.push(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn odf_header_lists_exg_columns_only() {
        let mut logger = DataLogger::new();
        let path = logger.start(LogFormat::ODF, 8, 250).expect("start odf");
        logger.log_sample(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], 0.0);
        logger
            .write_marker_annotation(0.12, "blink")
            .expect("marker");
        logger.stop();

        let mut body = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("Sample Rate: 250 Hz, Channels: 8"));
        assert!(body.contains("ch0,ch1,ch2,ch3,ch4,ch5,ch6,ch7"));
        assert!(!body.contains("timestamp,ch0"));
        assert!(body.contains("1.0000,2.0000,3.0000,4.0000,5.0000,6.0000,7.0000,8.0000"));
        assert!(body.contains("% MARKER,1,0.004000,blink"));
        let sidecar = crate::markers::load_sidecar(&path);
        assert_eq!(sidecar.len(), 1);
        assert_eq!(sidecar[0].sample_index, 1);
        assert_eq!(sidecar[0].label, "blink");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(crate::markers::sidecar_path(&path));
    }

    #[test]
    fn bdf_marker_is_within_one_sample() {
        let mut logger = DataLogger::new();
        let path = logger.start(LogFormat::BDF, 2, 250).expect("start bdf");
        for i in 0..250 {
            logger.log_sample(&[i as f64, 0.0], 0.0);
            if i == 124 {
                logger.write_marker_annotation(0.0, "mid").unwrap();
            }
        }
        logger.stop();
        let (samples, fs, n_exg, marks) =
            crate::data_writers::bdf::read_bdf(&path).expect("read bdf");
        assert_eq!(fs, 250);
        assert_eq!(n_exg, 2);
        assert_eq!(samples.len(), 250);
        assert!((samples[0][0] - 0.0).abs() < 2.0);
        let m = marks.iter().find(|m| m.label == "mid").expect("mid mark");
        assert!(
            m.sample_index.abs_diff(125) <= 1,
            "sample_index={} wanted ~125",
            m.sample_index
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(crate::markers::sidecar_path(&path));
    }
}
