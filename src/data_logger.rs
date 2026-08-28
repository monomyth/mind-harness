//! Data logging (ODF + BDF) — improved version.
//!
//! Supports both OpenBCI Data Format (text) and BDF+ (binary).

use crate::data_writers::bdf::DataWriterBDF;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

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
}

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
        }
    }

    pub fn start(
        &mut self,
        format: LogFormat,
        nb_channels: usize,
        sample_rate: i32,
    ) -> std::io::Result<PathBuf> {
        self.stop();

        std::fs::create_dir_all("Recordings")?;

        // Generate timestamped filename so we never overwrite previous recordings
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();

        let (path, _filename) = match format {
            LogFormat::ODF => {
                let filename = format!("OpenBCI_{}.txt", timestamp);
                let path = PathBuf::from("Recordings").join(&filename);
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
                let path = PathBuf::from("Recordings").join(&filename);
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
        self.sample_rate = sample_rate;
        self.recording_start = Some(std::time::Instant::now());

        // Write start annotation
        if let Some(ref mut bdf) = self.bdf_writer {
            let _ = bdf.write_annotation(0.0, 0.0, "Recording started");
        }

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
        let duration = self
            .recording_duration()
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        if let Some(mut bdf) = self.bdf_writer.take() {
            // Flush any remaining samples
            if !self.bdf_buffer.is_empty() && !self.bdf_buffer[0].is_empty() {
                let _ = bdf.write_data_record(&self.bdf_buffer);
            }

            // Write end annotation
            let _ = bdf.write_annotation(duration, 0.0, "Recording stopped");

            let _ = bdf.close();
        }
        self.odf_writer = None;
        self.output_path = None;
        self.rows_written = 0;
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

    /// Write a marker annotation into the current recording (BDF annotation channel
    /// or ODF comment). Called by WidgetContext when a marker is sent during an
    /// active recording. This makes markers first-class in both networking and
    /// saved data — exactly what Phase 4 + real experiments need.
    pub fn write_marker_annotation(&mut self, onset: f64, text: &str) -> std::io::Result<()> {
        let desc = format!("Marker: {}", text);
        if let Some(ref mut bdf) = self.bdf_writer {
            bdf.write_annotation(onset, 0.0, &desc)
        } else if let Some(ref mut w) = self.odf_writer {
            writeln!(w, "% {},{:.6}", desc, onset)?;
            Ok(())
        } else {
            Ok(())
        }
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
        assert!(body.contains("% Marker: blink,0.120000"));
        let _ = std::fs::remove_file(path);
    }
}
