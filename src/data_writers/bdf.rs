//! BDF+ (BioSemi Data Format) writer.
//! This is a more complete implementation aimed at producing files compatible
//! with EDFBrowser and other tools, modeled after the original Java DataWriterBDF.

use crate::markers::MarkerEvent;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const BDF_HEADER_SIZE: usize = 256;
const DIG_MIN: i32 = -8_388_608;
const DIG_MAX: i32 = 8_388_607;
const EXG_PHYS: i32 = 187_500;
const ACCEL_PHYS: i32 = 4;

#[derive(Clone, Debug)]
pub struct BdfSignal {
    pub label: String,
    pub transducer: String,
    pub dimension: String,
    pub phys_min: i32,
    pub phys_max: i32,
    pub dig_min: i32,
    pub dig_max: i32,
    pub prefilter: String,
}

impl BdfSignal {
    pub fn exg(i: usize) -> Self {
        Self {
            label: format!("EXG{i}"),
            transducer: "AgAgCl electrode".into(),
            dimension: "uV".into(),
            phys_min: -EXG_PHYS,
            phys_max: EXG_PHYS,
            dig_min: DIG_MIN,
            dig_max: DIG_MAX,
            prefilter: "HP:0.1Hz LP:75Hz".into(),
        }
    }

    pub fn accel(axis: &str) -> Self {
        Self {
            label: format!("Accel {axis}"),
            transducer: "Accelerometer".into(),
            dimension: "g".into(),
            phys_min: -ACCEL_PHYS,
            phys_max: ACCEL_PHYS,
            dig_min: DIG_MIN,
            dig_max: DIG_MAX,
            prefilter: String::new(),
        }
    }

    pub fn index() -> Self {
        Self {
            label: "Index".into(),
            transducer: String::new(),
            dimension: String::new(),
            phys_min: DIG_MIN,
            phys_max: DIG_MAX,
            dig_min: DIG_MIN,
            dig_max: DIG_MAX,
            prefilter: String::new(),
        }
    }

    pub fn is_index(&self) -> bool {
        self.label.trim().eq_ignore_ascii_case("Index")
    }

    pub fn to_digital(&self, sample: f64) -> i32 {
        if self.is_index() {
            return sample
                .round()
                .clamp(self.dig_min as f64, self.dig_max as f64) as i32;
        }
        if self.dimension.trim() == "uV" {
            let scale = DIG_MAX as f64 / EXG_PHYS as f64;
            return (sample * scale).clamp(self.dig_min as f64, self.dig_max as f64) as i32;
        }
        let span_p = (self.phys_max - self.phys_min) as f64;
        let span_d = (self.dig_max - self.dig_min) as f64;
        let d = (sample - self.phys_min as f64) / span_p.max(1.0) * span_d + self.dig_min as f64;
        d.round()
            .clamp(self.dig_min as f64, self.dig_max as f64) as i32
    }
}

/// 8 (or N) EXG + Accel X/Y/Z + packet/sample index. Annotations are added by the writer.
pub fn recording_signals(n_exg: usize) -> Vec<BdfSignal> {
    let mut out = Vec::with_capacity(n_exg + 4);
    for i in 0..n_exg {
        out.push(BdfSignal::exg(i));
    }
    out.push(BdfSignal::accel("X"));
    out.push(BdfSignal::accel("Y"));
    out.push(BdfSignal::accel("Z"));
    out.push(BdfSignal::index());
    out
}

fn pad_field(s: &str, n: usize) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    if v.len() > n {
        v.truncate(n);
    } else {
        v.resize(n, b' ');
    }
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BdfKind {
    Exg,
    Accel,
    Index,
    Annotation,
}

fn classify_label(label: &str) -> BdfKind {
    let t = label.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("annot") {
        BdfKind::Annotation
    } else if lower.starts_with("accel") {
        BdfKind::Accel
    } else if lower == "index" || lower.contains("package") || lower == "sample index" {
        BdfKind::Index
    } else {
        BdfKind::Exg
    }
}

pub struct DataWriterBDF {
    writer: BufWriter<File>,
    /// Phase 7: stored for header + future introspection / "current recording file" queries.
    /// Not read in the current hot path (written once at open).
    #[allow(dead_code)]
    fname: PathBuf,
    signals: Vec<BdfSignal>,
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
    pub fn new(path: PathBuf, signals: Vec<BdfSignal>, sample_rate: i32) -> std::io::Result<Self> {
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);

        let data_record_duration = 1.0;
        let samples_per_record = (sample_rate as f64 * data_record_duration) as usize;
        let nb_signals = signals.len();
        // Each sample is 3 bytes (24-bit) + 1 annotation channel (also 3 bytes per sample)
        let bytes_per_record = (nb_signals + 1) * samples_per_record * 3;

        let header_size = BDF_HEADER_SIZE + (nb_signals + 1) * BDF_HEADER_SIZE;

        Self::write_header(
            &mut writer,
            &signals,
            sample_rate,
            header_size,
            data_record_duration,
        )?;

        Ok(Self {
            writer,
            fname: path,
            signals,
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
        signals: &[BdfSignal],
        fs: i32,
        header_size: usize,
        duration: f64,
    ) -> std::io::Result<()> {
        let nb_signals = signals.len();
        // 1 byte: 0xFF
        w.write_all(&[0xFF])?;
        // 7 bytes: "BIOSEMI"
        w.write_all(b"BIOSEMI")?;

        w.write_all(&pad_field("OpenBCI Subject", 80))?;
        w.write_all(&pad_field("OpenBCI Rust GUI", 80))?;

        let now = chrono::Local::now();
        let dt = now.format("%d.%m.%y%H.%M.%S").to_string();
        w.write_all(&pad_field(&dt, 16))?;

        w.write_all(&pad_field(&format!("{header_size}"), 8))?;
        w.write_all(&pad_field("24BIT", 44))?;
        w.write_all(&pad_field("-1", 8))?;
        w.write_all(&pad_field(&format!("{duration}"), 8))?;

        let total_signals = nb_signals + 1;
        w.write_all(&pad_field(&format!("{total_signals}"), 4))?;

        for sig in signals {
            w.write_all(&pad_field(&sig.label, 16))?;
        }
        w.write_all(&pad_field("Annotations", 16))?;

        for sig in signals {
            w.write_all(&pad_field(&sig.transducer, 80))?;
        }
        w.write_all(&pad_field("", 80))?;

        for sig in signals {
            w.write_all(&pad_field(&sig.dimension, 8))?;
        }
        w.write_all(&pad_field("", 8))?;

        for sig in signals {
            w.write_all(&pad_field(&format!("{}", sig.phys_min), 8))?;
        }
        w.write_all(&pad_field("-1", 8))?;

        for sig in signals {
            w.write_all(&pad_field(&format!("{}", sig.phys_max), 8))?;
        }
        w.write_all(&pad_field("1", 8))?;

        for sig in signals {
            w.write_all(&pad_field(&format!("{}", sig.dig_min), 8))?;
        }
        w.write_all(&pad_field(&format!("{DIG_MIN}"), 8))?;

        for sig in signals {
            w.write_all(&pad_field(&format!("{}", sig.dig_max), 8))?;
        }
        w.write_all(&pad_field(&format!("{DIG_MAX}"), 8))?;

        for sig in signals {
            w.write_all(&pad_field(&sig.prefilter, 80))?;
        }
        w.write_all(&pad_field("", 80))?;

        let samples_per_record = fs;
        for _ in 0..nb_signals {
            w.write_all(&pad_field(&format!("{samples_per_record}"), 8))?;
        }
        w.write_all(&pad_field(&format!("{samples_per_record}"), 8))?;

        for _ in 0..total_signals {
            w.write_all(&pad_field("", 32))?;
        }

        Ok(())
    }

    /// Write one data record worth of data.
    /// `data` should be a slice of vectors, one per channel, each containing `sample_rate` samples.
    pub fn write_data_record(&mut self, data: &[Vec<f64>]) -> std::io::Result<()> {
        let nb_signals = self.signals.len();
        let n_samp = self.sample_rate as usize;
        for ch in 0..nb_signals {
            let channel_data = if ch < data.len() { &data[ch] } else { &vec![] };
            let sig = &self.signals[ch];

            for &sample in channel_data.iter().take(n_samp) {
                let digital = sig.to_digital(sample);
                let b0 = (digital & 0xFF) as u8;
                let b1 = ((digital >> 8) & 0xFF) as u8;
                let b2 = ((digital >> 16) & 0xFF) as u8;
                self.writer.write_all(&[b0, b1, b2])?;
            }

            let written = channel_data.len().min(n_samp);
            for _ in written..n_samp {
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
            let empty = vec![Vec::new(); self.signals.len()];
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

pub fn pack_annotation_record(
    record_start: f64,
    markers: &[(f64, String)],
    n_samples: usize,
) -> Vec<u8> {
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

    let kinds: Vec<BdfKind> = labels.iter().map(|l| classify_label(l)).collect();
    let n_exg = kinds.iter().filter(|k| **k == BdfKind::Exg).count().max(1);
    let fs = spr.first().copied().unwrap_or(250) as i32;
    let mut samples: Vec<Vec<f64>> = Vec::new();
    let mut markers = Vec::new();

    let records = if n_records == 0 {
        usize::MAX
    } else {
        n_records
    };
    for _ in 0..records {
        let mut rec_all: Vec<(BdfKind, Vec<f64>)> = Vec::with_capacity(n_signals);
        let mut eof = false;
        for ch in 0..n_signals {
            let n = spr.get(ch).copied().unwrap_or(fs as usize);
            let kind = kinds.get(ch).copied().unwrap_or(BdfKind::Exg);
            if kind == BdfKind::Annotation {
                let mut ann = vec![0u8; n * 3];
                if f.read_exact(&mut ann).is_err() {
                    eof = true;
                    break;
                }
                for (onset, label) in parse_tal_bytes(&ann) {
                    let idx = (onset * fs as f64).round().max(0.0) as u64;
                    let label = label.trim_start_matches("Marker: ").to_string();
                    if label != "Recording started" && label != "Recording stopped" {
                        markers.push(MarkerEvent::new(idx, onset, label));
                    }
                }
                continue;
            }
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
            rec_all.push((kind, col));
        }
        if eof {
            break;
        }
        let mut exg_ch = Vec::new();
        let mut accel_ch = Vec::new();
        let mut index_ch = Vec::new();
        for (kind, col) in rec_all {
            match kind {
                BdfKind::Exg => exg_ch.push(col),
                BdfKind::Accel => accel_ch.push(col),
                BdfKind::Index => index_ch.push(col),
                BdfKind::Annotation => {}
            }
        }
        if exg_ch.is_empty() {
            break;
        }
        let n_samp = exg_ch[0].len();
        for t in 0..n_samp {
            let mut row = Vec::with_capacity(exg_ch.len() + accel_ch.len() + index_ch.len());
            for ch in &exg_ch {
                row.push(ch.get(t).copied().unwrap_or(0.0));
            }
            for ch in &accel_ch {
                row.push(ch.get(t).copied().unwrap_or(0.0));
            }
            for ch in &index_ch {
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

    #[test]
    fn index_channel_identity_is_exact_integer() {
        let sig = BdfSignal::index();
        for n in [0.0, 1.0, 42.0, 255.0, 1024.0] {
            let d = sig.to_digital(n);
            assert_eq!(d, n as i32, "digital for {n}");
            let span = (sig.dig_max - sig.dig_min) as f64;
            let back = (d as f64 - sig.dig_min as f64) / span
                * (sig.phys_max - sig.phys_min) as f64
                + sig.phys_min as f64;
            assert!(
                (back - n).abs() < 1e-9,
                "identity roundtrip {n} -> {d} -> {back}"
            );
        }
    }

    #[test]
    fn recording_signals_are_eight_exg_accel_xyz_index() {
        let s = recording_signals(8);
        assert_eq!(s.len(), 12);
        assert_eq!(s[0].label, "EXG0");
        assert_eq!(s[7].label, "EXG7");
        assert_eq!(s[7].dimension, "uV");
        assert_eq!(s[8].label, "Accel X");
        assert_eq!(s[9].label, "Accel Y");
        assert_eq!(s[10].label, "Accel Z");
        assert_eq!(s[8].dimension, "g");
        assert_eq!(s[11].label, "Index");
        assert!(s[11].is_index());
    }
}
