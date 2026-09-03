//! Data logging (ODF + BDF) — improved version.
//!
//! Supports both OpenBCI Data Format (text) and BDF+ (binary).

use crate::data_writers::bdf::{recording_signals, DataWriterBDF};
use crate::markers::{self, MarkerEvent};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// One recorded sample: packet/sample index, up to 8 EXG, last-3 Accel.
#[derive(Clone, Debug, Default)]
pub struct RecordingSample {
    pub packet_index: f64,
    pub exg: Vec<f64>,
    pub accel: [f64; 3],
}

impl RecordingSample {
    pub fn from_board_row(
        row: &[f64],
        exg_channels: &[usize],
        accel_channels: &[usize],
        package_num_channel: Option<usize>,
        fallback_index: u64,
    ) -> Self {
        let exg = crate::board::extract_exg(row, exg_channels);
        let mut accel = [0.0; 3];
        for (i, &c) in accel_channels.iter().take(3).enumerate() {
            accel[i] = row.get(c).copied().unwrap_or(0.0);
        }
        let packet_index = package_num_channel
            .and_then(|i| row.get(i).copied())
            .unwrap_or(fallback_index as f64);
        Self {
            packet_index,
            exg,
            accel,
        }
    }

    /// Slice from tests / older callers: EXG only, or EXG+Accel+Index (BDF order).
    pub fn from_logger_slice(sample: &[f64], fallback_index: u64) -> Self {
        if sample.len() >= 12 {
            Self {
                packet_index: sample[11],
                exg: sample[..8].to_vec(),
                accel: [sample[8], sample[9], sample[10]],
            }
        } else if sample.len() >= 11 {
            Self {
                packet_index: fallback_index as f64,
                exg: sample[..8].to_vec(),
                accel: [sample[8], sample[9], sample[10]],
            }
        } else {
            Self {
                packet_index: fallback_index as f64,
                exg: sample.to_vec(),
                accel: [0.0; 3],
            }
        }
    }

    /// BDF column order: 8 EXG, Accel X/Y/Z, Index.
    pub fn bdf_row(&self, n_exg: usize) -> Vec<f64> {
        let mut row = vec![0.0; n_exg + 4];
        for (i, &v) in self.exg.iter().take(n_exg).enumerate() {
            row[i] = v;
        }
        row[n_exg] = self.accel[0];
        row[n_exg + 1] = self.accel[1];
        row[n_exg + 2] = self.accel[2];
        row[n_exg + 3] = self.packet_index;
        row
    }

    /// Original OpenBCI text: sample index, eight brain lines, last-3 Accel.
    pub fn odf_row(&self, n_exg: usize) -> Vec<f64> {
        let mut row = Vec::with_capacity(n_exg + 4);
        row.push(self.packet_index);
        for i in 0..n_exg {
            row.push(self.exg.get(i).copied().unwrap_or(0.0));
        }
        row.extend_from_slice(&self.accel);
        row
    }
}

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
    n_exg: usize,
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
            n_exg: 8,
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
                writeln!(file, "%OpenBCI Raw EEG Data")?;
                writeln!(file, "%Number of channels = {}", nb_channels)?;
                writeln!(file, "%Sample Rate = {} Hz", sample_rate)?;
                writeln!(file, "%First Column = SampleIndex")?;
                writeln!(file, "%Last 3 Columns = Accel Data (X, Y, Z)")?;
                let mut cols = vec!["Sample Index".to_string()];
                for i in 0..nb_channels {
                    cols.push(format!("EXG Channel {i}"));
                }
                cols.extend([
                    "Accel Channel 0".into(),
                    "Accel Channel 1".into(),
                    "Accel Channel 2".into(),
                ]);
                writeln!(file, "{}", cols.join(", "))?;
                self.odf_writer = Some(Box::new(file));
                self.output_path = Some(path.clone());
                (path, filename)
            }
            LogFormat::BDF => {
                let filename = format!("OpenBCI_{}.bdf", timestamp);
                let path = rec_dir.join(&filename);
                let signals = recording_signals(nb_channels);
                let n_sig = signals.len();
                let bdf = DataWriterBDF::new(path.clone(), signals, sample_rate)?;
                self.bdf_writer = Some(bdf);
                self.output_path = Some(path.clone());
                self.samples_per_record = sample_rate as usize;
                self.bdf_buffer = vec![Vec::new(); n_sig];
                (path, filename)
            }
        };

        self.format = format;
        self.rows_written = 0;
        self.samples_logged = 0;
        self.n_exg = nb_channels;
        self.markers.clear();
        self.sample_rate = sample_rate;
        self.recording_start = Some(std::time::Instant::now());

        // Experiment markers go to TAL / ODF; do not queue bookkeeping text that
        // would force an extra empty BDF record on close.

        Ok(path)
    }

    pub fn log_recording(&mut self, rec: &RecordingSample) {
        match self.format {
            LogFormat::ODF => {
                if let Some(ref mut w) = self.odf_writer {
                    let row = rec.odf_row(self.n_exg);
                    for (i, val) in row.iter().enumerate() {
                        if i > 0 {
                            let _ = write!(w, ", ");
                        }
                        if i == 0 {
                            let _ = write!(w, "{:.0}", val);
                        } else {
                            let _ = write!(w, "{:.4}", val);
                        }
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

                let row = rec.bdf_row(self.n_exg);
                for (i, &val) in row.iter().enumerate() {
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

    pub fn log_sample(&mut self, sample: &[f64], _timestamp: f64) {
        let rec = RecordingSample::from_logger_slice(sample, self.samples_logged);
        self.log_recording(&rec);
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

/// File I/O for a recording. The ingest thread only `send`s; the writer thread
/// owns `DataLogger` and waits on disk.
enum RecordCmd {
    Start {
        format: LogFormat,
        n_exg: usize,
        sample_rate: i32,
        reply: Sender<std::io::Result<PathBuf>>,
    },
    Sample(RecordingSample),
    Marker(String),
    Stop {
        reply: Sender<()>,
    },
    Shutdown,
}

/// Cloneable enqueue path. `send` never waits on disk.
#[derive(Clone)]
pub struct RecordSampleTx {
    tx: Sender<RecordCmd>,
}

impl RecordSampleTx {
    pub fn send(&self, rec: RecordingSample) {
        let _ = self.tx.send(RecordCmd::Sample(rec));
    }
}

/// UI-facing recording facade. Header/body/markers run on `mind-harness-record`.
pub struct RecordPump {
    tx: Sender<RecordCmd>,
    join: Option<JoinHandle<()>>,
    logging: bool,
    samples_enqueued: u64,
    path: Option<PathBuf>,
    recording_start: Option<std::time::Instant>,
    markers: Vec<MarkerEvent>,
    sample_rate: i32,
}

impl RecordPump {
    pub fn spawn() -> Self {
        Self::spawn_inner(Duration::ZERO)
    }

    #[cfg(test)]
    pub fn spawn_with_write_delay(delay: Duration) -> Self {
        Self::spawn_inner(delay)
    }

    fn spawn_inner(write_delay: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("mind-harness-record".into())
            .spawn(move || record_writer_loop(rx, DataLogger::new(), write_delay))
            .expect("spawn mind-harness-record");
        Self {
            tx,
            join: Some(join),
            logging: false,
            samples_enqueued: 0,
            path: None,
            recording_start: None,
            markers: Vec::new(),
            sample_rate: 250,
        }
    }

    pub fn sample_tx(&self) -> RecordSampleTx {
        RecordSampleTx {
            tx: self.tx.clone(),
        }
    }

    pub fn start(
        &mut self,
        format: LogFormat,
        nb_channels: usize,
        sample_rate: i32,
    ) -> std::io::Result<PathBuf> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(RecordCmd::Start {
                format,
                n_exg: nb_channels,
                sample_rate,
                reply: reply_tx,
            })
            .map_err(|_| std::io::Error::other("record writer died"))?;
        let path = reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| std::io::Error::other("record start timeout"))?;
        let path = path?;
        self.logging = true;
        self.samples_enqueued = 0;
        self.path = Some(path.clone());
        self.recording_start = Some(std::time::Instant::now());
        self.markers.clear();
        self.sample_rate = sample_rate;
        Ok(path)
    }

    pub fn log_recording(&mut self, rec: &RecordingSample) {
        if !self.logging {
            return;
        }
        self.samples_enqueued += 1;
        let _ = self.tx.send(RecordCmd::Sample(rec.clone()));
    }

    pub fn stop(&mut self) {
        if !self.logging {
            return;
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        let _ = self.tx.send(RecordCmd::Stop { reply: reply_tx });
        let _ = reply_rx.recv_timeout(Duration::from_secs(30));
        self.logging = false;
        self.recording_start = None;
    }

    pub fn is_logging(&self) -> bool {
        self.logging
    }

    pub fn recording_duration(&self) -> Option<std::time::Duration> {
        self.recording_start.map(|t| t.elapsed())
    }

    pub fn current_file(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    pub fn samples_logged(&self) -> u64 {
        self.samples_enqueued
    }

    pub fn markers(&self) -> &[MarkerEvent] {
        &self.markers
    }

    pub fn write_marker_annotation(&mut self, _onset_unix: f64, text: &str) -> std::io::Result<()> {
        if !self.logging || text.trim().is_empty() {
            return Ok(());
        }
        let event = MarkerEvent::new(self.samples_enqueued, self.board_time(), text.trim());
        let _ = self.tx.send(RecordCmd::Marker(event.label.clone()));
        self.markers.push(event);
        Ok(())
    }

    fn board_time(&self) -> f64 {
        self.samples_enqueued as f64 / (self.sample_rate.max(1) as f64)
    }
}

impl Drop for RecordPump {
    fn drop(&mut self) {
        let _ = self.tx.send(RecordCmd::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn record_writer_loop(rx: Receiver<RecordCmd>, mut logger: DataLogger, write_delay: Duration) {
    loop {
        let wait_t0 = Instant::now();
        let cmd = match rx.recv() {
            Ok(c) => c,
            Err(_) => {
                logger.stop();
                break;
            }
        };
        let wait = wait_t0.elapsed();
        let busy_t0 = Instant::now();
        match cmd {
            RecordCmd::Start {
                format,
                n_exg,
                sample_rate,
                reply,
            } => {
                let _ = reply.send(logger.start(format, n_exg, sample_rate));
            }
            RecordCmd::Sample(rec) => {
                if !write_delay.is_zero() {
                    thread::sleep(write_delay);
                }
                if logger.is_logging() {
                    logger.log_recording(&rec);
                }
            }
            RecordCmd::Marker(text) => {
                let _ = logger.write_marker_annotation(0.0, &text);
            }
            RecordCmd::Stop { reply } => {
                logger.stop();
                let _ = reply.send(());
            }
            RecordCmd::Shutdown => {
                logger.stop();
                crate::starve::SPLIT.add_file(wait + busy_t0.elapsed(), busy_t0.elapsed());
                break;
            }
        }
        crate::starve::SPLIT.add_file(wait + busy_t0.elapsed(), busy_t0.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn odf_is_sample_index_eight_brain_last3_accel() {
        let mut logger = DataLogger::new();
        let path = logger.start(LogFormat::ODF, 8, 250).expect("start odf");
        let rec = RecordingSample {
            packet_index: 42.0,
            exg: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
            accel: [0.1, -0.2, 0.9],
        };
        logger.log_recording(&rec);
        logger
            .write_marker_annotation(0.12, "blink")
            .expect("marker");
        logger.stop();

        let mut body = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("%First Column = SampleIndex"));
        assert!(body.contains("%Last 3 Columns = Accel Data (X, Y, Z)"));
        assert!(body.contains("Sample Index, EXG Channel 0"));
        assert!(body.contains("Accel Channel 2"));
        assert!(!body.contains("timestamp,ch0"));
        assert!(body.contains("42, 1.0000, 2.0000, 3.0000, 4.0000, 5.0000, 6.0000, 7.0000, 8.0000, 0.1000, -0.2000, 0.9000"));
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

    #[test]
    fn bdf_keeps_accel_and_packet_index_as_extra_channels() {
        let mut logger = DataLogger::new();
        let path = logger.start(LogFormat::BDF, 8, 250).expect("start bdf");
        for i in 0..256 {
            let rec = RecordingSample {
                packet_index: i as f64,
                exg: vec![i as f64; 8],
                accel: [0.01 * i as f64, -0.02, 0.98],
            };
            logger.log_recording(&rec);
        }
        logger.stop();
        let (samples, fs, n_exg, _) =
            crate::data_writers::bdf::read_bdf(&path).expect("read bdf");
        assert_eq!(fs, 250);
        assert_eq!(n_exg, 8);
        assert!(
            samples.len() >= 256,
            "need the 256 written samples (BDF pads the last record), got {}",
            samples.len()
        );
        assert_eq!(samples[0].len(), 12, "8 EXG + 3 Accel + Index");
        let idx_col = 11;
        for i in [0usize, 1, 42, 255] {
            let got = samples[i][idx_col];
            assert!(
                (got - i as f64).abs() < 0.51,
                "packet index sample {i} got {got}, BDF must hold packet numbers"
            );
        }
        assert!((samples[10][8] - 0.10).abs() < 0.02, "Accel X");
        assert!((samples[10][10] - 0.98).abs() < 0.02, "Accel Z");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(crate::markers::sidecar_path(&path));
    }

    #[test]
    fn record_pump_enqueue_does_not_wait_on_slow_writer() {
        let pump = RecordPump::spawn_with_write_delay(Duration::from_millis(20));
        let rec = RecordingSample {
            packet_index: 1.0,
            exg: vec![0.0; 8],
            accel: [0.0; 3],
        };
        let t0 = std::time::Instant::now();
        let tx = pump.sample_tx();
        for _ in 0..20 {
            tx.send(rec.clone());
        }
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_millis(80),
            "20 samples onto a 20ms/sample writer must not block the caller, took {elapsed:?}"
        );
    }

    #[test]
    fn record_pump_writes_bdf_on_the_writer_thread() {
        let mut pump = RecordPump::spawn();
        let path = pump.start(LogFormat::BDF, 8, 250).expect("start");
        for i in 0..250 {
            pump.log_recording(&RecordingSample {
                packet_index: i as f64,
                exg: vec![i as f64; 8],
                accel: [0.0, 0.0, 1.0],
            });
        }
        pump.write_marker_annotation(0.0, "sit still").unwrap();
        pump.stop();
        let (samples, fs, n_exg, marks) =
            crate::data_writers::bdf::read_bdf(&path).expect("read");
        assert_eq!(fs, 250);
        assert_eq!(n_exg, 8);
        assert!(
            samples.len() >= 250,
            "writer thread must flush the 250 samples, got {}",
            samples.len()
        );
        assert!(
            marks.iter().any(|m| m.label == "sit still"),
            "writer-thread TAL must keep the file mark, got {marks:?}"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(crate::markers::sidecar_path(&path));
    }
}
