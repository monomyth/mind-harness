//! Starve sidecar: package-index loss vs wall-clock Hertz vs this-process CPU.
//! Not on the glass. A slow UI frame is not packet loss.
//!
//! Periodic lines split ingest vs file I/O vs UI tick so a Record-start hitch
//! is not mistaken for radio loss.

use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

/// Cyton package index is u8 wrapping. 255→0 is not a gap. 10→14 skipped 11,12,13.
pub fn packet_gap_u8(prev: u8, curr: u8) -> u64 {
    curr.wrapping_sub(prev).wrapping_sub(1) as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketGapAccount {
    pub n: u64,
    pub last: Option<u8>,
    pub max_gap: u64,
    pub lost: u64,
}

/// Packet-gap accounting without hardware. First sample is received, not a gap.
pub fn account_packet_gaps_u8(indices: &[u8]) -> PacketGapAccount {
    let mut acc = PacketGapAccount {
        n: 0,
        last: None,
        max_gap: 0,
        lost: 0,
    };
    for &idx in indices {
        acc.n += 1;
        if let Some(prev) = acc.last {
            let gap = packet_gap_u8(prev, idx);
            acc.lost += gap;
            if gap > acc.max_gap {
                acc.max_gap = gap;
            }
        }
        acc.last = Some(idx);
    }
    acc
}

pub fn accel_triple_nonzero(xyz: [f64; 3]) -> bool {
    xyz.iter().any(|&v| v != 0.0)
}

pub fn accel_any_nonzero(triples: &[[f64; 3]]) -> bool {
    triples.iter().copied().any(accel_triple_nonzero)
}

/// Board rows + accel column indices. Any axis ≠ 0 this interval means radio Accel lived.
pub fn accel_any_from_rows(rows: &[Vec<f64>], accel_channels: &[usize]) -> bool {
    rows.iter().any(|row| {
        let mut xyz = [0.0; 3];
        for (i, &c) in accel_channels.iter().take(3).enumerate() {
            xyz[i] = row.get(c).copied().unwrap_or(0.0);
        }
        accel_triple_nonzero(xyz)
    })
}

fn ns_to_ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

fn add_ns(slot: &AtomicU64, d: Duration) {
    let n = d.as_nanos().min(u128::from(u64::MAX)) as u64;
    slot.fetch_add(n, Ordering::Relaxed);
}

/// Cross-thread split timings. Ingest, record writer, and UI stamp atomics; tick() takes them.
pub struct StarveSplit {
    ingest_elapsed_ns: AtomicU64,
    ingest_busy_ns: AtomicU64,
    file_elapsed_ns: AtomicU64,
    file_busy_ns: AtomicU64,
    ui_elapsed_ns: AtomicU64,
    ui_busy_ns: AtomicU64,
    last_packet_index: AtomicI64,
    max_gap: AtomicU64,
    accel_nonzero: AtomicBool,
}

#[derive(Clone, Copy, Debug)]
pub struct SplitSnap {
    pub ingest_elapsed_ms: f64,
    pub ingest_busy_ms: f64,
    pub file_elapsed_ms: f64,
    pub file_busy_ms: f64,
    pub ui_elapsed_ms: f64,
    pub ui_busy_ms: f64,
    pub packet_index: Option<u64>,
    pub gap_size: Option<u64>,
    pub accel_nonzero: bool,
}

impl StarveSplit {
    pub const fn new() -> Self {
        Self {
            ingest_elapsed_ns: AtomicU64::new(0),
            ingest_busy_ns: AtomicU64::new(0),
            file_elapsed_ns: AtomicU64::new(0),
            file_busy_ns: AtomicU64::new(0),
            ui_elapsed_ns: AtomicU64::new(0),
            ui_busy_ns: AtomicU64::new(0),
            last_packet_index: AtomicI64::new(-1),
            max_gap: AtomicU64::new(0),
            accel_nonzero: AtomicBool::new(false),
        }
    }

    pub fn add_ingest(&self, elapsed: Duration, busy: Duration) {
        add_ns(&self.ingest_elapsed_ns, elapsed);
        add_ns(&self.ingest_busy_ns, busy);
    }

    pub fn add_file(&self, elapsed: Duration, busy: Duration) {
        add_ns(&self.file_elapsed_ns, elapsed);
        add_ns(&self.file_busy_ns, busy);
    }

    pub fn add_ui(&self, elapsed: Duration, busy: Duration) {
        add_ns(&self.ui_elapsed_ns, elapsed);
        add_ns(&self.ui_busy_ns, busy);
    }

    pub fn note_packet(&self, index: u64, gap: u64) {
        self.last_packet_index
            .store(index.min(i64::MAX as u64) as i64, Ordering::Relaxed);
        let mut cur = self.max_gap.load(Ordering::Relaxed);
        while gap > cur {
            match self.max_gap.compare_exchange_weak(
                cur,
                gap,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
    }

    pub fn note_accel_nonzero(&self) {
        self.accel_nonzero.store(true, Ordering::Relaxed);
    }

    pub fn take(&self) -> SplitSnap {
        let pkt = self.last_packet_index.swap(-1, Ordering::Relaxed);
        let gap = self.max_gap.swap(0, Ordering::Relaxed);
        SplitSnap {
            ingest_elapsed_ms: ns_to_ms(self.ingest_elapsed_ns.swap(0, Ordering::Relaxed)),
            ingest_busy_ms: ns_to_ms(self.ingest_busy_ns.swap(0, Ordering::Relaxed)),
            file_elapsed_ms: ns_to_ms(self.file_elapsed_ns.swap(0, Ordering::Relaxed)),
            file_busy_ms: ns_to_ms(self.file_busy_ns.swap(0, Ordering::Relaxed)),
            ui_elapsed_ms: ns_to_ms(self.ui_elapsed_ns.swap(0, Ordering::Relaxed)),
            ui_busy_ms: ns_to_ms(self.ui_busy_ns.swap(0, Ordering::Relaxed)),
            packet_index: if pkt < 0 { None } else { Some(pkt as u64) },
            gap_size: if gap == 0 { None } else { Some(gap) },
            accel_nonzero: self.accel_nonzero.swap(false, Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        let _ = self.take();
    }
}

/// Process-wide split counters. Debug sidecar only.
pub static SPLIT: StarveSplit = StarveSplit::new();

pub struct UiTickGuard {
    t0: Instant,
    elapsed: Duration,
}

impl Drop for UiTickGuard {
    fn drop(&mut self) {
        SPLIT.add_ui(self.elapsed, self.t0.elapsed());
    }
}

pub fn begin_ui_tick(frame_dt_s: f32) -> UiTickGuard {
    UiTickGuard {
        t0: Instant::now(),
        elapsed: Duration::from_secs_f64(f64::from(frame_dt_s.max(0.0))),
    }
}

fn wall_unix_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Serialize, Clone)]
struct StarveLine {
    t_s: f64,
    wall_unix_s: f64,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    packet_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap_size: Option<u64>,
    accel_nonzero: bool,
    ingest_elapsed_ms: f64,
    ingest_busy_ms: f64,
    file_elapsed_ms: f64,
    file_busy_ms: f64,
    ui_elapsed_ms: f64,
    ui_busy_ms: f64,
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
        SPLIT.reset();
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
        let snap = SPLIT.take();
        let line = StarveLine {
            t_s,
            wall_unix_s: wall_unix_s(),
            delivered,
            lost,
            index_loss_pct,
            wall_hz,
            nominal_hz,
            cpu_pct,
            ingest_empty,
            layer: layer.as_str().to_string(),
            get_err,
            packet_index: snap.packet_index,
            gap_size: snap.gap_size,
            accel_nonzero: snap.accel_nonzero,
            ingest_elapsed_ms: snap.ingest_elapsed_ms,
            ingest_busy_ms: snap.ingest_busy_ms,
            file_elapsed_ms: snap.file_elapsed_ms,
            file_busy_ms: snap.file_busy_ms,
            ui_elapsed_ms: snap.ui_elapsed_ms,
            ui_busy_ms: snap.ui_busy_ms,
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

    #[test]
    fn packet_gap_u8_consecutive_and_wrap_are_zero() {
        assert_eq!(packet_gap_u8(10, 11), 0);
        assert_eq!(packet_gap_u8(255, 0), 0);
        assert_eq!(packet_gap_u8(0, 1), 0);
    }

    #[test]
    fn packet_gap_u8_skips_count_without_hardware() {
        assert_eq!(packet_gap_u8(10, 14), 3);
        assert_eq!(packet_gap_u8(250, 2), 7); // 251..255, 0, 1
    }

    #[test]
    fn account_packet_gaps_reports_last_and_max() {
        let a = account_packet_gaps_u8(&[10, 11, 14, 15]);
        assert_eq!(a.n, 4);
        assert_eq!(a.last, Some(15));
        // 11→14 missed 12,13 (same wrap rule as SampleIndexTracker).
        assert_eq!(a.max_gap, 2);
        assert_eq!(a.lost, 2);
        assert_eq!(packet_gap_u8(10, 14), 3);
        let wrap = account_packet_gaps_u8(&[254, 255, 0, 1]);
        assert_eq!(wrap.lost, 0);
        assert_eq!(wrap.max_gap, 0);
        assert_eq!(account_packet_gaps_u8(&[]).last, None);
    }

    #[test]
    fn accel_all_zero_is_dead_this_interval() {
        assert!(!accel_triple_nonzero([0.0, 0.0, 0.0]));
        assert!(!accel_any_nonzero(&[[0.0; 3], [0.0; 3]]));
        assert!(!accel_any_from_rows(
            &[vec![1.0, 0.0, 0.0, 0.0], vec![2.0, 0.0, 0.0, 0.0]],
            &[1, 2, 3]
        ));
    }

    #[test]
    fn accel_one_nonzero_sample_is_detected() {
        assert!(accel_triple_nonzero([0.0, 0.02, 0.0]));
        assert!(accel_any_nonzero(&[[0.0; 3], [0.0, 0.0, -0.1]]));
        assert!(accel_any_from_rows(
            &[vec![1.0, 0.0, 0.0, 0.0], vec![2.0, 0.0, 0.4, 0.0]],
            &[1, 2, 3]
        ));
    }

    #[test]
    fn jsonl_fields_split_ingest_file_ui() {
        let line = StarveLine {
            t_s: 1.0,
            wall_unix_s: 1_700_000_000.0,
            delivered: 250,
            lost: 3,
            index_loss_pct: 1.2,
            wall_hz: 250.0,
            nominal_hz: 250.0,
            cpu_pct: 8.0,
            ingest_empty: false,
            layer: "ok".into(),
            get_err: None,
            packet_index: Some(14),
            gap_size: Some(3),
            accel_nonzero: true,
            ingest_elapsed_ms: 1000.0,
            ingest_busy_ms: 12.0,
            file_elapsed_ms: 1000.0,
            file_busy_ms: 4.0,
            ui_elapsed_ms: 1000.0,
            ui_busy_ms: 40.0,
        };
        let s = serde_json::to_string(&line).unwrap();
        for key in [
            "wall_unix_s",
            "delivered",
            "packet_index",
            "gap_size",
            "accel_nonzero",
            "ingest_elapsed_ms",
            "ingest_busy_ms",
            "file_elapsed_ms",
            "file_busy_ms",
            "ui_elapsed_ms",
            "ui_busy_ms",
            "cpu_pct",
        ] {
            assert!(s.contains(&format!("\"{key}\"")), "{s}");
        }
        let zero_gap = StarveLine {
            gap_size: None,
            packet_index: None,
            ..line
        };
        let s0 = serde_json::to_string(&zero_gap).unwrap();
        assert!(!s0.contains("gap_size"), "{s0}");
        assert!(!s0.contains("packet_index"), "{s0}");
    }
}
