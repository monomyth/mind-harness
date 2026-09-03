//! Starve sidecar: package-index loss vs wall-clock Hertz vs this-process CPU.
//! Not on the glass. A slow UI frame is not packet loss.

use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Radio,
    Mac,
    Ingest,
    Mixed,
    Ok,
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Radio => "radio",
            Layer::Mac => "mac",
            Layer::Ingest => "ingest",
            Layer::Mixed => "mixed",
            Layer::Ok => "ok",
        }
    }
}

/// Prove which layer dropped packets. Do not guess Bluetooth.
pub fn classify_layer(
    index_loss_pct: f64,
    wall_hz: f64,
    nominal_hz: f64,
    cpu_pct: f64,
) -> Layer {
    let hz_floor = 0.85 * nominal_hz.max(1.0);
    if index_loss_pct < 2.0 && wall_hz >= hz_floor {
        Layer::Ok
    } else if index_loss_pct >= 20.0 && cpu_pct < 40.0 {
        Layer::Radio
    } else if index_loss_pct < 2.0 && wall_hz < hz_floor && cpu_pct >= 40.0 {
        Layer::Mac
    } else if index_loss_pct < 2.0 && wall_hz < hz_floor && cpu_pct < 40.0 {
        Layer::Ingest
    } else {
        Layer::Mixed
    }
}

#[derive(Serialize)]
struct StarveLine {
    t_s: f64,
    delivered: u64,
    lost: u64,
    index_loss_pct: f64,
    wall_hz: f64,
    nominal_hz: f64,
    cpu_pct: f64,
    ingest_empty: bool,
    layer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    get_err: Option<String>,
}

pub struct StarveLog {
    t0: Option<Instant>,
    last_cpu_sec: Option<f64>,
    fallback_name: Option<PathBuf>,
}

impl Default for StarveLog {
    fn default() -> Self {
        Self {
            t0: None,
            last_cpu_sec: None,
            fallback_name: None,
        }
    }
}

impl StarveLog {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Snapshot CPU at the start of a ~1s window so the first line is not cpu_pct=0.
    pub fn note_window_start(&mut self) {
        if self.t0.is_none() {
            self.t0 = Some(Instant::now());
        }
        self.last_cpu_sec = process_cpu_seconds();
    }

    pub fn tick(
        &mut self,
        delivered: u64,
        lost: u64,
        elapsed: f64,
        empty_ticks: u64,
        nominal_hz: f64,
        recording: Option<&Path>,
        get_err: Option<String>,
    ) {
        if elapsed <= 0.05 {
            return;
        }
        let now = Instant::now();
        let t0 = *self.t0.get_or_insert(now);
        let t_s = now.saturating_duration_since(t0).as_secs_f64();
        let cpu_now = process_cpu_seconds();
        let cpu_pct = match (self.last_cpu_sec, cpu_now) {
            (Some(prev), Some(cur)) if cur >= prev => {
                ((cur - prev) / elapsed * 100.0).clamp(0.0, 400.0)
            }
            _ => 0.0,
        };
        self.last_cpu_sec = cpu_now;
        let index_loss_pct = crate::stream_stats::loss_percent(delivered, lost);
        let wall_hz = delivered as f64 / elapsed;
        let ingest_empty = empty_ticks > 0;
        let layer = classify_layer(index_loss_pct, wall_hz, nominal_hz, cpu_pct);
        let line = StarveLine {
            t_s,
            delivered,
            lost,
            index_loss_pct,
            wall_hz,
            nominal_hz,
            cpu_pct,
            ingest_empty,
            layer: layer.as_str().to_string(),
            get_err,
        };
        let path = sidecar_path(recording, &mut self.fallback_name);
        let _ = append_line(&path, &line);
    }
}

pub fn sidecar_path(recording: Option<&Path>, fallback: &mut Option<PathBuf>) -> PathBuf {
    if let Some(rec) = recording {
        let stem = rec.with_extension("");
        return PathBuf::from(format!("{}.starve.jsonl", stem.display()));
    }
    if fallback.is_none() {
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let dir = crate::data_logger::recordings_dir();
        *fallback = Some(dir.join(format!("starve_{stamp}.jsonl")));
    }
    fallback.clone().expect("fallback set")
}

fn append_line(path: &Path, line: &StarveLine) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let s = serde_json::to_string(line).map_err(std::io::Error::other)?;
    writeln!(f, "{s}")?;
    Ok(())
}

fn process_cpu_seconds() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if rc != 0 {
            return None;
        }
        let u = unsafe { usage.assume_init() };
        let user = u.ru_utime.tv_sec as f64 + u.ru_utime.tv_usec as f64 / 1e6;
        let sys = u.ru_stime.tv_sec as f64 + u.ru_stime.tv_usec as f64 / 1e6;
        Some(user + sys)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn layer_radio_is_index_gaps_with_quiet_cpu() {
        assert_eq!(classify_layer(92.2, 20.0, 250.0, 8.0), Layer::Radio);
    }

    #[test]
    fn layer_mac_is_wall_slow_with_busy_cpu_and_no_gaps() {
        assert_eq!(classify_layer(0.4, 80.0, 250.0, 70.0), Layer::Mac);
    }

    #[test]
    fn layer_ingest_is_wall_slow_quiet_cpu_no_gaps() {
        assert_eq!(classify_layer(0.1, 40.0, 250.0, 5.0), Layer::Ingest);
    }

    #[test]
    fn layer_ok_is_full_rate_and_low_loss() {
        assert_eq!(classify_layer(0.4, 249.0, 250.0, 12.0), Layer::Ok);
    }

    #[test]
    fn layer_mixed_when_loss_and_cpu_both_high() {
        assert_eq!(classify_layer(50.0, 80.0, 250.0, 80.0), Layer::Mixed);
    }

    #[test]
    fn sidecar_follows_recording_stem() {
        let mut fb = None;
        let p = sidecar_path(Some(Path::new("/tmp/OpenBCI_x.bdf")), &mut fb);
        assert!(p.to_string_lossy().ends_with("OpenBCI_x.starve.jsonl"));
    }

    #[test]
    fn sidecar_fallback_uses_recordings_dir_stamp() {
        let mut fb = None;
        let p = sidecar_path(None, &mut fb);
        let name = p.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("starve_"), "{name}");
        assert!(name.ends_with(".jsonl"), "{name}");
        assert_eq!(sidecar_path(None, &mut fb), p);
    }

    #[test]
    fn layer_table_boundaries() {
        assert_eq!(classify_layer(20.0, 250.0, 250.0, 39.9), Layer::Radio);
        assert_eq!(classify_layer(20.0, 250.0, 250.0, 40.0), Layer::Mixed);
        assert_eq!(classify_layer(1.9, 212.5, 250.0, 10.0), Layer::Ok);
        assert_eq!(classify_layer(1.9, 212.4, 250.0, 10.0), Layer::Ingest);
        assert_eq!(classify_layer(1.9, 212.4, 250.0, 40.0), Layer::Mac);
    }
}
