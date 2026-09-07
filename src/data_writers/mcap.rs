//! Foxglove MCAP session archive (JSON messages, no zstd).
//!
//! Topics: `/eeg/sample` (one JSON object per board sample) and `/markers`.
//! Timestamps are session-relative nanoseconds. File I/O stays on the record thread.

use crate::data_logger::RecordingSample;
use crate::markers::MarkerEvent;
use mcap::records::{MessageHeader, Metadata};
use mcap::{MessageStream, WriteOptions, Writer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

pub const TOPIC_EEG: &str = "/eeg/sample";
pub const TOPIC_MARKERS: &str = "/markers";

const EEG_SCHEMA: &str = r#"{
  "type": "object",
  "title": "mind_harness.EegSample",
  "properties": {
    "sample_index": { "type": "integer" },
    "exg": { "type": "array", "items": { "type": "number" } },
    "accel": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
    "time": { "type": "number" },
    "analog": { "type": "array", "items": { "type": "number" } },
    "digital": { "type": "array", "items": { "type": "number" } }
  },
  "required": ["sample_index", "exg", "accel", "time"]
}"#;

const MARKER_SCHEMA: &str = r#"{
  "type": "object",
  "title": "mind_harness.Marker",
  "properties": {
    "sample_index": { "type": "integer" },
    "board_timestamp": { "type": "number" },
    "label": { "type": "string" }
  },
  "required": ["sample_index", "board_timestamp", "label"]
}"#;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct EegSampleMsg {
    sample_index: i64,
    exg: Vec<f64>,
    accel: [f64; 3],
    time: f64,
    #[serde(default)]
    analog: Vec<f64>,
    #[serde(default)]
    digital: Vec<f64>,
}

fn mcap_err(e: mcap::McapError) -> io::Error {
    io::Error::other(e.to_string())
}

fn json_err(e: serde_json::Error) -> io::Error {
    io::Error::other(e.to_string())
}

fn time_ns(t: f64) -> u64 {
    if !t.is_finite() || t < 0.0 {
        0
    } else {
        (t * 1_000_000_000.0).round() as u64
    }
}

fn pad_aux(src: &[f64], n: usize) -> Vec<f64> {
    let mut v = vec![0.0; n];
    for (i, slot) in v.iter_mut().enumerate() {
        *slot = src.get(i).copied().unwrap_or(0.0);
    }
    v
}

fn sample_time(rec: &RecordingSample, fallback: f64) -> f64 {
    if rec.time.is_finite() && rec.time != 0.0 {
        rec.time
    } else {
        fallback
    }
}

pub struct DataWriterMcap {
    writer: Option<Writer<File>>,
    n_exg: usize,
    n_analog: usize,
    n_digital: usize,
    eeg_channel: u16,
    marker_channel: u16,
    eeg_sequence: u32,
    marker_sequence: u32,
}

impl DataWriterMcap {
    pub fn new(
        path: PathBuf,
        n_exg: usize,
        sample_rate: i32,
        n_analog: usize,
        n_digital: usize,
    ) -> io::Result<Self> {
        let n_exg = n_exg.max(1);
        let file = File::create(&path)?;
        let opts = WriteOptions::new()
            .compression(None)
            .library(format!("mind-harness {}", env!("CARGO_PKG_VERSION")));
        let mut writer = Writer::with_options(file, opts).map_err(mcap_err)?;

        let eeg_schema = writer
            .add_schema(
                "mind_harness.EegSample",
                "jsonschema",
                EEG_SCHEMA.as_bytes(),
            )
            .map_err(mcap_err)?;
        let marker_schema = writer
            .add_schema(
                "mind_harness.Marker",
                "jsonschema",
                MARKER_SCHEMA.as_bytes(),
            )
            .map_err(mcap_err)?;

        let mut ch_meta = BTreeMap::new();
        ch_meta.insert("sample_rate".into(), sample_rate.to_string());
        ch_meta.insert("n_exg".into(), n_exg.to_string());
        ch_meta.insert("n_analog".into(), n_analog.to_string());
        ch_meta.insert("n_digital".into(), n_digital.to_string());
        ch_meta.insert("mind_harness".into(), env!("CARGO_PKG_VERSION").into());

        let eeg_channel = writer
            .add_channel(eeg_schema, TOPIC_EEG, "json", &ch_meta)
            .map_err(mcap_err)?;
        let marker_channel = writer
            .add_channel(marker_schema, TOPIC_MARKERS, "json", &BTreeMap::new())
            .map_err(mcap_err)?;

        writer
            .write_metadata(&Metadata {
                name: "mind_harness".into(),
                metadata: ch_meta,
            })
            .map_err(mcap_err)?;

        Ok(Self {
            writer: Some(writer),
            n_exg,
            n_analog,
            n_digital,
            eeg_channel,
            marker_channel,
            eeg_sequence: 0,
            marker_sequence: 0,
        })
    }

    pub fn write_sample(&mut self, rec: &RecordingSample, fallback_time: f64) -> io::Result<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("mcap writer closed"))?;
        let time = sample_time(rec, fallback_time);
        let msg = EegSampleMsg {
            sample_index: rec.packet_index.round() as i64,
            exg: pad_aux(&rec.exg, self.n_exg),
            accel: rec.accel,
            time,
            analog: pad_aux(&rec.analog, self.n_analog),
            digital: pad_aux(&rec.digital, self.n_digital),
        };
        let data = serde_json::to_vec(&msg).map_err(json_err)?;
        let ns = time_ns(time);
        let seq = self.eeg_sequence;
        self.eeg_sequence = self.eeg_sequence.wrapping_add(1);
        writer
            .write_to_known_channel(
                &MessageHeader {
                    channel_id: self.eeg_channel,
                    sequence: seq,
                    log_time: ns,
                    publish_time: ns,
                },
                &data,
            )
            .map_err(mcap_err)
    }

    pub fn write_marker(&mut self, event: &MarkerEvent) -> io::Result<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("mcap writer closed"))?;
        let data = serde_json::to_vec(event).map_err(json_err)?;
        let ns = time_ns(event.board_timestamp);
        let seq = self.marker_sequence;
        self.marker_sequence = self.marker_sequence.wrapping_add(1);
        writer
            .write_to_known_channel(
                &MessageHeader {
                    channel_id: self.marker_channel,
                    sequence: seq,
                    log_time: ns,
                    publish_time: ns,
                },
                &data,
            )
            .map_err(mcap_err)
    }

    pub fn close(&mut self) -> io::Result<()> {
        if let Some(mut w) = self.writer.take() {
            w.finish().map_err(mcap_err)?;
        }
        Ok(())
    }
}

/// One decoded MCAP take (rows as `RecordingSample`).
#[derive(Clone, Debug)]
pub struct McapRecording {
    pub samples: Vec<RecordingSample>,
    pub markers: Vec<MarkerEvent>,
    pub sample_rate: i32,
    pub n_exg: usize,
    pub n_analog: usize,
    pub n_digital: usize,
}

fn meta_i32(meta: &BTreeMap<String, String>, key: &str, default: i32) -> i32 {
    meta.get(key)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

pub fn read_mcap(path: &Path) -> io::Result<McapRecording> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < mcap::MAGIC.len() || !bytes.starts_with(mcap::MAGIC) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an MCAP file (missing magic)",
        ));
    }
    let mut samples = Vec::new();
    let mut markers = Vec::new();
    let mut sample_rate = 250;
    let mut n_exg = 0usize;
    let mut n_analog = 0usize;
    let mut n_digital = 0usize;
    for msg in MessageStream::new(&bytes).map_err(mcap_err)? {
        let msg = msg.map_err(mcap_err)?;
        match msg.channel.topic.as_str() {
            TOPIC_EEG => {
                sample_rate = meta_i32(&msg.channel.metadata, "sample_rate", sample_rate).max(1);
                n_exg = meta_i32(&msg.channel.metadata, "n_exg", n_exg as i32).max(0) as usize;
                n_analog =
                    meta_i32(&msg.channel.metadata, "n_analog", n_analog as i32).max(0) as usize;
                n_digital =
                    meta_i32(&msg.channel.metadata, "n_digital", n_digital as i32).max(0) as usize;
                let body: EegSampleMsg = serde_json::from_slice(&msg.data).map_err(json_err)?;
                if n_exg == 0 {
                    n_exg = body.exg.len().max(1);
                }
                samples.push(RecordingSample {
                    packet_index: body.sample_index as f64,
                    exg: body.exg,
                    accel: body.accel,
                    time: body.time,
                    analog: body.analog,
                    digital: body.digital,
                });
            }
            TOPIC_MARKERS => {
                let event: MarkerEvent = serde_json::from_slice(&msg.data).map_err(json_err)?;
                markers.push(event);
            }
            _ => {}
        }
    }
    if samples.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mcap contained no EEG samples",
        ));
    }
    Ok(McapRecording {
        samples,
        markers,
        sample_rate,
        n_exg: n_exg.max(1),
        n_analog,
        n_digital,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mh220_{tag}_{}.mcap",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn sample(i: i64) -> RecordingSample {
        RecordingSample {
            packet_index: i as f64,
            exg: vec![i as f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            accel: [0.01 * i as f64, -0.02, 0.98],
            time: i as f64 / 250.0,
            analog: vec![],
            digital: vec![],
        }
    }

    #[test]
    fn magic_and_roundtrip_index_exg_accel_time() {
        let path = temp_path("round");
        let mut w = DataWriterMcap::new(path.clone(), 8, 250, 0, 0).unwrap();
        for i in 0..20 {
            w.write_sample(&sample(i), i as f64 / 250.0).unwrap();
        }
        w.write_marker(&MarkerEvent::new(10, 0.04, "blink"))
            .unwrap();
        w.close().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.starts_with(mcap::MAGIC),
            "MCAP files start with \\x89MCAP0\\r\\n"
        );
        assert!(
            bytes.ends_with(mcap::MAGIC),
            "MCAP files end with the same magic"
        );

        let got = read_mcap(&path).unwrap();
        assert_eq!(got.sample_rate, 250);
        assert_eq!(got.n_exg, 8);
        assert_eq!(got.n_analog, 0);
        assert_eq!(got.n_digital, 0);
        assert_eq!(got.samples.len(), 20);
        assert_eq!(got.samples[10].packet_index, 10.0);
        assert!((got.samples[10].exg[0] - 10.0).abs() < 1e-9);
        assert!((got.samples[10].accel[2] - 0.98).abs() < 1e-9);
        assert!((got.samples[10].time - 10.0 / 250.0).abs() < 1e-12);
        assert_eq!(got.markers.len(), 1);
        assert_eq!(got.markers[0].label, "blink");
        assert_eq!(got.markers[0].sample_index, 10);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn analog_mode_writes_analog_array() {
        let path = temp_path("analog");
        let mut w = DataWriterMcap::new(path.clone(), 8, 250, 3, 0).unwrap();
        let mut rec = sample(3);
        rec.analog = vec![1.5, 2.5, 3.5];
        w.write_sample(&rec, 0.012).unwrap();
        w.close().unwrap();
        let got = read_mcap(&path).unwrap();
        assert_eq!(got.n_analog, 3);
        assert_eq!(got.samples[0].analog, vec![1.5, 2.5, 3.5]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn digital_mode_writes_digital_array() {
        let path = temp_path("digital");
        let mut w = DataWriterMcap::new(path.clone(), 8, 250, 0, 5).unwrap();
        let mut rec = sample(1);
        rec.digital = vec![1.0, 0.0, 1.0, 0.0, 1.0];
        w.write_sample(&rec, 0.004).unwrap();
        w.close().unwrap();
        let got = read_mcap(&path).unwrap();
        assert_eq!(got.n_digital, 5);
        assert_eq!(got.samples[0].digital, vec![1.0, 0.0, 1.0, 0.0, 1.0]);
        let _ = std::fs::remove_file(path);
    }
}
