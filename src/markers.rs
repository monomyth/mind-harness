//! Sample-accurate experiment markers (ODF comment / sidecar / BDF TAL).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkerEvent {
    pub sample_index: u64,
    pub board_timestamp: f64,
    pub label: String,
}

impl MarkerEvent {
    pub fn new(sample_index: u64, board_timestamp: f64, label: impl Into<String>) -> Self {
        Self {
            sample_index,
            board_timestamp,
            label: label.into(),
        }
    }

    pub fn odf_line(&self) -> String {
        format!(
            "% MARKER,{},{:.6},{}",
            self.sample_index, self.board_timestamp, self.label
        )
    }
}

/// `% MARKER,<sample_index>,<board_timestamp>,<label>` (Rust) or `% Marker: text,unix`.
pub fn parse_odf_marker_line(line: &str) -> Option<MarkerEvent> {
    let t = line.trim().trim_start_matches('%').trim();
    if t.to_ascii_uppercase().starts_with("MARKER,") {
        let rest = t.split_once(',')?.1;
        let mut parts = rest.splitn(3, ',');
        let idx = parts.next()?.trim().parse().ok()?;
        let ts = parts.next()?.trim().parse().ok()?;
        let label = parts.next().unwrap_or("").trim().to_string();
        if label.is_empty() {
            return None;
        }
        return Some(MarkerEvent::new(idx, ts, label));
    }
    if let Some(rest) = t.strip_prefix("Marker:") {
        let rest = rest.trim();
        let (label, ts) = rest.rsplit_once(',')?;
        let unix: f64 = ts.trim().parse().ok()?;
        return Some(MarkerEvent::new(0, unix, label.trim()));
    }
    None
}

pub fn load_sidecar(recording: &Path) -> Vec<MarkerEvent> {
    let path = sidecar_path(recording);
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<MarkerEvent>(l.trim()).ok())
        .collect()
}

pub fn sidecar_path(recording: &Path) -> std::path::PathBuf {
    let stem = recording.with_extension("");
    Path::new(&format!("{}.markers.jsonl", stem.display())).to_path_buf()
}

pub fn append_sidecar(recording: &Path, marker: &MarkerEvent) -> std::io::Result<()> {
    use std::io::Write;
    let path = sidecar_path(recording);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(marker).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odf_roundtrip_line() {
        let m = MarkerEvent::new(42, 0.168, "blink");
        let parsed = parse_odf_marker_line(&m.odf_line()).unwrap();
        assert_eq!(parsed.sample_index, 42);
        assert!((parsed.board_timestamp - 0.168).abs() < 1e-6);
        assert_eq!(parsed.label, "blink");
    }
}
