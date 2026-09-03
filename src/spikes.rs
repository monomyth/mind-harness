//! All-eight common-mode wall sidecar. Off the glass — not a Head Plot bead.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpikeEvent {
    pub t: f64,
    pub dv: [f64; 8],
    pub mag: f64,
    pub accel: [f64; 3],
    pub packet_index: u64,
}

pub fn sidecar_path(recording: &Path) -> PathBuf {
    let stem = recording.with_extension("");
    PathBuf::from(format!("{}.spikes.jsonl", stem.display()))
}

pub fn append(recording: &Path, event: &SpikeEvent) -> std::io::Result<PathBuf> {
    let path = sidecar_path(recording);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_sits_beside_the_recording() {
        let rec = Path::new("/tmp/OpenBCI_demo.bdf");
        assert_eq!(
            sidecar_path(rec),
            PathBuf::from("/tmp/OpenBCI_demo.spikes.jsonl")
        );
    }

    #[test]
    fn all_eight_wall_line_has_t_dv_mag_accel_packet() {
        let dir = std::env::temp_dir().join("mind_harness_spikes");
        let _ = std::fs::create_dir_all(&dir);
        let rec = dir.join("wall.bdf");
        let side = sidecar_path(&rec);
        let _ = std::fs::remove_file(&side);
        let ev = SpikeEvent {
            t: 1.25,
            dv: [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0],
            mag: 17.0,
            accel: [0.1, 0.2, 0.9],
            packet_index: 42,
        };
        append(&rec, &ev).expect("write");
        let body = std::fs::read_to_string(&side).expect("read");
        let got: SpikeEvent = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert!((got.t - 1.25).abs() < 1e-9);
        assert_eq!(got.dv[7], 17.0);
        assert!((got.mag - 17.0).abs() < 1e-9);
        assert_eq!(got.accel, [0.1, 0.2, 0.9]);
        assert_eq!(got.packet_index, 42);
        let _ = std::fs::remove_file(&side);
    }
}
