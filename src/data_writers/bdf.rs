//! BDF+ (BioSemi Data Format) writer.
//! This is a more complete implementation aimed at producing files compatible
//! with EDFBrowser and other tools, modeled after the original Java DataWriterBDF.

use crate::markers::MarkerEvent;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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
    pending_ann: Vec<(f64, String)>,
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
            pending_ann: Vec::new(),
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

        let rec_start = self.records_written as f64;
        let rec_end = rec_start + 1.0;
        let mut here = Vec::new();
        let mut later = Vec::new();
        for (onset, text) in self.pending_ann.drain(..) {
            if onset >= rec_start && onset < rec_end {
                here.push((onset, text));
            } else if onset >= rec_end {
                later.push((onset, text));
            } else {
                here.push((onset, text));
            }
        }
        self.pending_ann = later;
        let packed = pack_annotation_record(rec_start, &here, self.sample_rate as usize);
        self.writer.write_all(&packed)?;

        self.records_written += 1;
        Ok(())
    }

    pub fn close(&mut self) -> std::io::Result<()> {
        if !self.pending_ann.is_empty() {
            let empty = vec![Vec::new(); self.nb_signals];
            let _ = self.write_data_record(&empty);
        }
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

    /// Queue a BDF+ TAL for the data record covering `onset_sec`.
    pub fn write_annotation(
        &mut self,
        onset_sec: f64,
        _duration: f64,
        description: &str,
    ) -> std::io::Result<()> {
        if description.is_empty() {
            return Ok(());
        }
        self.pending_ann.push((onset_sec, description.to_string()));
        Ok(())
    }
}

const TAL_SEP: u8 = 0x14;
const TAL_END: u8 = 0x00;

pub fn encode_tal(record_start: f64, markers: &[(f64, String)]) -> Vec<u8> {
    let mut out = format!("+{record_start:.6}").into_bytes();
    out.push(TAL_SEP);
    out.push(TAL_SEP);
    out.push(TAL_END);
    for (onset, text) in markers {
        out.extend(format!("+{onset:.6}").into_bytes());
        out.push(TAL_SEP);
        out.extend(text.as_bytes());
        out.push(TAL_SEP);
        out.push(TAL_END);
    }
    out
}

pub fn pack_annotation_record(record_start: f64, markers: &[(f64, String)], n_samples: usize) -> Vec<u8> {
    let mut bytes = encode_tal(record_start, markers);
    let need = n_samples * 3;
    bytes.resize(need, 0);
    bytes
}

pub fn parse_tal_bytes(bytes: &[u8]) -> Vec<(f64, String)> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for chunk in text.split('\u{0}') {
        if chunk.is_empty() {
            continue;
        }
        let parts: Vec<&str> = chunk.split('\u{14}').collect();
        if parts.is_empty() {
            continue;
        }
        let onset = parts[0].trim().trim_start_matches('+').parse::<f64>().ok();
        let Some(onset) = onset else {
            continue;
        };
        let label = parts
            .iter()
            .skip(1)
            .find(|s| !s.is_empty())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if !label.is_empty() {
            out.push((onset, label));
        }
    }
    out
}

fn parse_header_i32(s: &str) -> i32 {
    s.trim().parse().unwrap_or(0)
}

fn read_exact_str(f: &mut File, n: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn i24_le(b0: u8, b1: u8, b2: u8) -> i32 {
    let mut v = i32::from(b0) | (i32::from(b1) << 8) | (i32::from(b2) << 16);
    if v & 0x0080_0000 != 0 {
        v |= !0x00FF_FFFF;
    }
    v
}

/// Parse a BDF this app writes (24-bit, 1 s records, last signal = Annotations).
type BdfRecording = (Vec<Vec<f64>>, i32, usize, Vec<MarkerEvent>);

pub fn read_bdf(path: &Path) -> std::io::Result<BdfRecording> {
    let mut f = File::open(path)?;
    let mut ver = [0u8; 8];
    f.read_exact(&mut ver)?;
    if ver[0] != 0xFF {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a BDF file (missing 0xFF)",
        ));
    }
    let _patient = read_exact_str(&mut f, 80)?;
    let _recording = read_exact_str(&mut f, 80)?;
    let _dt = read_exact_str(&mut f, 16)?;
    let header_bytes: usize = parse_header_i32(&read_exact_str(&mut f, 8)?) as usize;
    let _reserved = read_exact_str(&mut f, 44)?;
    let n_records = parse_header_i32(&read_exact_str(&mut f, 8)?).max(0) as usize;
    let _duration = read_exact_str(&mut f, 8)?;
    let n_signals = parse_header_i32(&read_exact_str(&mut f, 4)?).max(1) as usize;

    let mut labels = Vec::with_capacity(n_signals);
    for _ in 0..n_signals {
        labels.push(read_exact_str(&mut f, 16)?);
    }
    for _ in 0..n_signals {
        let _ = read_exact_str(&mut f, 80)?;
    }
    let mut phys_dim = Vec::new();
    for _ in 0..n_signals {
        phys_dim.push(read_exact_str(&mut f, 8)?);
    }
    let mut phys_min = Vec::new();
    let mut phys_max = Vec::new();
    let mut dig_min = Vec::new();
    let mut dig_max = Vec::new();
    for _ in 0..n_signals {
        phys_min.push(parse_header_i32(&read_exact_str(&mut f, 8)?) as f64);
    }
    for _ in 0..n_signals {
        phys_max.push(parse_header_i32(&read_exact_str(&mut f, 8)?) as f64);
    }
    for _ in 0..n_signals {
        dig_min.push(parse_header_i32(&read_exact_str(&mut f, 8)?) as f64);
    }
    for _ in 0..n_signals {
        dig_max.push(parse_header_i32(&read_exact_str(&mut f, 8)?) as f64);
    }
    for _ in 0..n_signals {
        let _ = read_exact_str(&mut f, 80)?;
    }
    let mut spr = Vec::new();
    for _ in 0..n_signals {
        spr.push(parse_header_i32(&read_exact_str(&mut f, 8)?).max(1) as usize);
    }
    for _ in 0..n_signals {
        let _ = read_exact_str(&mut f, 32)?;
    }

    let header_size = 256 + n_signals * 256;
    if header_bytes > 0 && f.stream_position()? < header_size as u64 {
        f.seek(SeekFrom::Start(header_size as u64))?;
    }

    let n_exg = n_signals.saturating_sub(1).max(1);
    let fs = spr.first().copied().unwrap_or(250) as i32;
    let mut samples: Vec<Vec<f64>> = Vec::new();
    let mut markers = Vec::new();

    let records = if n_records == 0 {
        usize::MAX
    } else {
        n_records
    };
    for _ in 0..records {
        let mut rec_ch: Vec<Vec<f64>> = Vec::with_capacity(n_exg);
        let mut eof = false;
        for ch in 0..n_exg {
            let n = spr.get(ch).copied().unwrap_or(fs as usize);
            let mut col = Vec::with_capacity(n);
            for _ in 0..n {
                let mut b = [0u8; 3];
                if f.read_exact(&mut b).is_err() {
                    eof = true;
                    break;
                }
                let d = i24_le(b[0], b[1], b[2]) as f64;
                let pmin = phys_min.get(ch).copied().unwrap_or(-187500.0);
                let pmax = phys_max.get(ch).copied().unwrap_or(187500.0);
                let dmin = dig_min.get(ch).copied().unwrap_or(-8388608.0);
                let dmax = dig_max.get(ch).copied().unwrap_or(8388607.0);
                let span = (dmax - dmin).abs().max(1.0);
                col.push((d - dmin) / span * (pmax - pmin) + pmin);
            }
            if eof {
                break;
            }
            rec_ch.push(col);
        }
        if eof {
            break;
        }
        let ann_n = spr.get(n_exg).copied().unwrap_or(fs as usize);
        let mut ann = vec![0u8; ann_n * 3];
        if f.read_exact(&mut ann).is_err() {
            break;
        }
        for (onset, label) in parse_tal_bytes(&ann) {
            let idx = (onset * fs as f64).round().max(0.0) as u64;
            let label = label.trim_start_matches("Marker: ").to_string();
            if label != "Recording started" && label != "Recording stopped" {
                markers.push(MarkerEvent::new(idx, onset, label));
            }
        }
        if rec_ch.is_empty() {
            break;
        }
        let n_samp = rec_ch[0].len();
        for t in 0..n_samp {
            let mut row = Vec::with_capacity(n_exg);
            for ch in &rec_ch {
                row.push(ch.get(t).copied().unwrap_or(0.0));
            }
            samples.push(row);
        }
    }

    if samples.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "BDF contained no data records",
        ));
    }
    let _ = (labels, phys_dim);
    Ok((samples, fs, n_exg, markers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tal_roundtrip_keeps_label_and_onset() {
        let packed = pack_annotation_record(0.0, &[(0.168, "blink".into())], 64);
        let got = parse_tal_bytes(&packed);
        assert_eq!(got.len(), 1);
        assert!((got[0].0 - 0.168).abs() < 1e-4);
        assert_eq!(got[0].1, "blink");
    }
}
