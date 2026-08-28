//! BDF+ (BioSemi Data Format) writer.
//! This is a more complete implementation aimed at producing files compatible
//! with EDFBrowser and other tools, modeled after the original Java DataWriterBDF.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::PathBuf;

const BDF_HEADER_SIZE: usize = 256;

pub struct DataWriterBDF {
    writer: BufWriter<File>,
    /// Phase 7: stored for header + future introspection / "current recording file" queries.
    /// Not read in the current hot path (written once at open).
    #[allow(dead_code)]
    fname: PathBuf,
    nb_signals: usize, // number of real signals (not counting annotations)
    sample_rate: i32,
    records_written: i64, // can be -1 during writing
    /// Phase 7: BDF record geometry (written to header, kept for potential duration / size queries).
    #[allow(dead_code)]
    data_record_duration: f64, // usually 1.0 second
    #[allow(dead_code)]
    bytes_per_record: usize,
    #[allow(dead_code)]
    header_size: usize,
}

impl DataWriterBDF {
    pub fn new(path: PathBuf, nb_signals: usize, sample_rate: i32) -> std::io::Result<Self> {
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);

        let data_record_duration = 1.0;
        let samples_per_record = (sample_rate as f64 * data_record_duration) as usize;
        // Each sample is 3 bytes (24-bit) + 1 annotation channel (also 3 bytes per sample for simplicity in this MVP)
        let bytes_per_record = (nb_signals + 1) * samples_per_record * 3;

        let header_size = BDF_HEADER_SIZE + (nb_signals + 1) * BDF_HEADER_SIZE;

        Self::write_header(
            &mut writer,
            nb_signals,
            sample_rate,
            header_size,
            data_record_duration,
        )?;

        Ok(Self {
            writer,
            fname: path,
            nb_signals,
            sample_rate,
            records_written: 0,
            data_record_duration,
            bytes_per_record,
            header_size,
        })
    }

    fn write_header(
        w: &mut BufWriter<File>,
        nb_signals: usize,
        fs: i32,
        header_size: usize,
        duration: f64,
    ) -> std::io::Result<()> {
        // 1 byte: 0xFF
        w.write_all(&[0xFF])?;
        // 7 bytes: "BIOSEMI"
        w.write_all(b"BIOSEMI")?;

        // 80 bytes: Local patient identification
        let patient = format!("{: <80}", "OpenBCI Subject");
        w.write_all(patient.as_bytes())?;

        // 80 bytes: Local recording identification
        let recording = format!("{: <80}", "OpenBCI Rust GUI");
        w.write_all(recording.as_bytes())?;

        // 16 bytes: Start date + time (dd.mm.yyHH.MM.SS)
        let now = chrono::Local::now();
        let dt = now.format("%d.%m.%y%H.%M.%S").to_string();
        w.write_all(format!("{: <16}", dt).as_bytes())?;

        // 8 bytes: Number of bytes in header
        w.write_all(format!("{: <8}", header_size).as_bytes())?;

        // 44 bytes: Reserved
        w.write_all(format!("{: <44}", "24BIT").as_bytes())?;

        // 8 bytes: Number of data records (-1 = unknown)
        w.write_all(format!("{: <8}", -1i32).as_bytes())?;

        // 8 bytes: Duration of a data record in seconds
        w.write_all(format!("{: <8}", duration).as_bytes())?;

        // 4 bytes: Number of signals (including annotations)
        let total_signals = nb_signals + 1;
        w.write_all(format!("{: <4}", total_signals).as_bytes())?;

        // --- Per-signal headers (256 bytes each) ---

        // Labels (16 bytes each)
        for i in 0..nb_signals {
            let label = format!("EXG{: <13}", i);
            w.write_all(label.as_bytes())?;
        }
        w.write_all(format!("{: <16}", "Annotations").as_bytes())?;

        // Transducer type (80 bytes each)
        for _ in 0..total_signals {
            w.write_all(format!("{: <80}", "AgAgCl electrode").as_bytes())?;
        }

        // Physical dimension (8 bytes each)
        for _ in 0..nb_signals {
            w.write_all(format!("{: <8}", "uV").as_bytes())?;
        }
        w.write_all(format!("{: <8}", "").as_bytes())?; // annotations

        // Physical minimum (8 bytes each)
        for _ in 0..nb_signals {
            w.write_all(format!("{: <8}", -187500).as_bytes())?;
        }
        w.write_all(format!("{: <8}", -1).as_bytes())?;

        // Physical maximum (8 bytes each)
        for _ in 0..nb_signals {
            w.write_all(format!("{: <8}", 187500).as_bytes())?;
        }
        w.write_all(format!("{: <8}", 1).as_bytes())?;

        // Digital minimum (8 bytes each)
        for _ in 0..nb_signals {
            w.write_all(format!("{: <8}", -8388608).as_bytes())?;
        }
        w.write_all(format!("{: <8}", -8388608).as_bytes())?;

        // Digital maximum (8 bytes each)
        for _ in 0..nb_signals {
            w.write_all(format!("{: <8}", 8388607).as_bytes())?;
        }
        w.write_all(format!("{: <8}", 8388607).as_bytes())?;

        // Prefiltering (80 bytes each) - simplified
        for _ in 0..total_signals {
            w.write_all(format!("{: <80}", "HP:0.1Hz LP:75Hz").as_bytes())?;
        }

        // Samples per data record (8 bytes each)
        let samples_per_record = fs; // 1 second records
        for _ in 0..nb_signals {
            w.write_all(format!("{: <8}", samples_per_record).as_bytes())?;
        }
        w.write_all(format!("{: <8}", samples_per_record).as_bytes())?; // annotations also get space

        // Reserved per signal (32 bytes each)
        for _ in 0..total_signals {
            w.write_all(format!("{: <32}", "").as_bytes())?;
        }

        Ok(())
    }

    /// Write one data record worth of data.
    /// `data` should be a slice of vectors, one per channel, each containing `sample_rate` samples.
    pub fn write_data_record(&mut self, data: &[Vec<f64>]) -> std::io::Result<()> {
        if data.len() != self.nb_signals {
            // For simplicity we just pad or truncate
        }

        for ch in 0..self.nb_signals {
            let channel_data = if ch < data.len() { &data[ch] } else { &vec![] };

            for &sample in channel_data.iter().take(self.sample_rate as usize) {
                // Cyton Java GUI: physical ±187500 µV, digital ±8388607 (gain=24).
                let scale = 8388607.0 / 187500.0;
                let digital = (sample * scale).clamp(-8388608.0, 8388607.0) as i32;

                let b0 = (digital & 0xFF) as u8;
                let b1 = ((digital >> 8) & 0xFF) as u8;
                let b2 = ((digital >> 16) & 0xFF) as u8;
                self.writer.write_all(&[b0, b1, b2])?;
            }

            // Fill remaining samples with zeros if we didn't get enough
            let written = channel_data.len().min(self.sample_rate as usize);
            for _ in written..self.sample_rate as usize {
                self.writer.write_all(&[0, 0, 0])?;
            }
        }

        // Write annotation channel (very simplified - just zeros for now)
        for _ in 0..self.sample_rate {
            self.writer.write_all(&[0, 0, 0])?;
        }

        self.records_written += 1;
        Ok(())
    }

    pub fn close(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;

        // Go back and write the real number of records
        let mut file = self.writer.get_ref().try_clone()?;
        file.seek(SeekFrom::Start(236))?; // position of "number of data records" in header
        let records_str = format!("{: <8}", self.records_written);
        file.write_all(records_str.as_bytes())?;

        Ok(())
    }

    /// Phase 7: accessor retained for potential "show current recording path" or debug UI.
    /// Not called in the main app loop yet (harmless).
    #[allow(dead_code)]
    pub fn filename(&self) -> &PathBuf {
        &self.fname
    }

    /// Write a BDF+ annotation (for start/stop events, markers, etc.)
    pub fn write_annotation(
        &mut self,
        onset_sec: f64,
        duration: f64,
        description: &str,
    ) -> std::io::Result<()> {
        // Simplified BDF+ annotation record
        // In a full implementation this would go into the annotation channel properly.
        // For now we log it and can expand later.
        tracing::info!(
            "BDF Annotation @ {:.2}s (+{:.2}s): {}",
            onset_sec,
            duration,
            description
        );
        Ok(())
    }
}
