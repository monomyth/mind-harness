//! Contact-lost sidecar: square-step vs neighbors. Never DC, never p2p as a reason.

use crate::laterality::{contact_snapshot, latch_rails, SiteContact};
use crate::markers::MarkerEvent;
use crate::widgets::head_plot::LABELS;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const RULE: &str = "square-step vs neighbors";
const RING_SEC: f64 = 2.0;
const MARK_RANGE_SEC: f64 = 10.0;
const RATE_NOMINAL: f64 = 250.0;
const RATE_OFF_HZ: f64 = 5.0;
const LOSS_UP_PCT: f64 = 0.5;
/// Last half-second of a take that already has a Recording-stopped mark is the stop click, not a site.
const STOP_TAIL_SEC: f64 = 0.5;

#[derive(Clone, Debug, Serialize)]
pub struct ContactEvent {
    pub event: String,
    pub sample: usize,
    pub t_s: f64,
    pub site: String,
    pub max_step: f64,
    pub neighbor_med: f64,
    pub ratio: f64,
    pub p2p: f64,
    pub loss_pct: f64,
    pub sr_hz: f64,
    pub rate_off: bool,
    pub loss_up: bool,
    pub mark: Option<String>,
    pub rule: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel_s: Option<f64>,
}

impl ContactEvent {
    fn line(
        event: &str,
        sample: usize,
        t_s: f64,
        site: &str,
        nums: SiteNums,
        loss_pct: f64,
        sr_hz: f64,
        mark: Option<String>,
        rel_s: Option<f64>,
    ) -> Self {
        Self {
            event: event.to_string(),
            sample,
            t_s,
            site: site.to_string(),
            max_step: nums.max_step,
            neighbor_med: nums.neighbor_med,
            ratio: nums.ratio,
            p2p: nums.p2p,
            loss_pct,
            sr_hz,
            rate_off: (sr_hz - RATE_NOMINAL).abs() > RATE_OFF_HZ,
            loss_up: loss_pct > LOSS_UP_PCT,
            mark,
            rule: RULE.to_string(),
            rel_s,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SiteNums {
    max_step: f64,
    neighbor_med: f64,
    p2p: f64,
    ratio: f64,
}

impl From<SiteContact> for SiteNums {
    fn from(s: SiteContact) -> Self {
        Self {
            max_step: s.max_step,
            neighbor_med: s.neighbor_med,
            p2p: s.p2p,
            ratio: s.ratio,
        }
    }
}

#[derive(Clone, Debug)]
struct RingFrame {
    sample: usize,
    t_s: f64,
    sites: [SiteNums; 8],
    loss_pct: f64,
    sr_hz: f64,
}

struct PendingAfter {
    idx: usize,
    latch_t_s: f64,
}

pub struct ContactLog {
    latched: [bool; 8],
    ring: VecDeque<RingFrame>,
    pending_after: Vec<PendingAfter>,
    last_common_t: f64,
    common_notice: Option<String>,
}

impl Default for ContactLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ContactLog {
    pub fn new() -> Self {
        Self {
            latched: [false; 8],
            ring: VecDeque::new(),
            pending_after: Vec::new(),
            last_common_t: f64::NEG_INFINITY,
            common_notice: None,
        }
    }

    pub fn reset(&mut self) {
        self.latched = [false; 8];
        self.ring.clear();
        self.pending_after.clear();
        self.last_common_t = f64::NEG_INFINITY;
        self.common_notice = None;
    }

    pub fn take_common_mode_notice(&mut self) -> Option<String> {
        self.common_notice.take()
    }

    pub fn observe(
        &mut self,
        channels: &[Vec<f64>],
        sample: usize,
        t_s: f64,
        sr_hz: f64,
        loss_pct: f64,
        markers: &[MarkerEvent],
        recording: Option<&Path>,
    ) {
        if channels.is_empty() {
            return;
        }
        let snap = contact_snapshot(channels);
        let mut sites = [SiteNums::default(); 8];
        for (i, s) in snap.iter().enumerate() {
            sites[i] = SiteNums::from(*s);
        }
        self.ring.push_back(RingFrame {
            sample,
            t_s,
            sites,
            loss_pct,
            sr_hz,
        });
        while let Some(front) = self.ring.front() {
            if t_s - front.t_s > RING_SEC + 1e-9 {
                self.ring.pop_front();
            } else {
                break;
            }
        }

        let prev = self.latched;
        // Do not latch the Recording-stopped click — that mark is the operator ending the take.
        if !after_recording_stopped(markers, t_s) {
            latch_rails(&mut self.latched, channels);
        }

        if let Some((n_jump, mag)) = crate::laterality::common_mode_jump(channels) {
            if t_s - self.last_common_t > 1.0 {
                self.last_common_t = t_s;
                let mark = nearest_mark(markers, t_s);
                let vs = if n_jump >= 8 { "all eight" } else { "many vs one" };
                let line = format!(
                    "common-mode jump t={t_s:.2}s n={n_jump} {vs} mag={mag:.0}uV loss={loss_pct:.1}% mark={}",
                    mark.as_deref().unwrap_or("-")
                );
                self.common_notice = Some(line);
                if let Some(path) = recording {
                    let ev = ContactEvent::line(
                        "common_mode",
                        sample,
                        t_s,
                        "all",
                        SiteNums {
                            max_step: mag,
                            neighbor_med: mag,
                            p2p: 0.0,
                            ratio: 1.0,
                        },
                        loss_pct,
                        sr_hz,
                        mark,
                        None,
                    );
                    let _ = append_event(path, &ev);
                }
            }
        }


        let Some(path) = recording else {
            return;
        };

        for i in 0..8 {
            if self.latched[i] && !prev[i] {
                let mark = nearest_mark(markers, t_s);
                let ev = ContactEvent::line(
                    "latch", sample, t_s, LABELS[i], sites[i], loss_pct, sr_hz, mark, None,
                );
                let _ = append_event(path, &ev);
                self.dump_ring(path, i, t_s, markers, false);
                self.pending_after.push(PendingAfter {
                    idx: i,
                    latch_t_s: t_s,
                });
            } else if !self.latched[i] && prev[i] {
                let mark = nearest_mark(markers, t_s);
                let ev = ContactEvent::line(
                    "unlatch", sample, t_s, LABELS[i], sites[i], loss_pct, sr_hz, mark, None,
                );
                let _ = append_event(path, &ev);
            }
        }

        let due: Vec<PendingAfter> = self.pending_after.drain(..).collect();
        let mut still = Vec::new();
        for p in due {
            if t_s + 1e-9 >= p.latch_t_s + 1.0 {
                self.dump_ring(path, p.idx, p.latch_t_s, markers, true);
            } else {
                still.push(p);
            }
        }
        self.pending_after = still;
    }

    fn dump_ring(
        &self,
        path: &Path,
        idx: usize,
        latch_t_s: f64,
        markers: &[MarkerEvent],
        after_only: bool,
    ) {
        for frame in &self.ring {
            let rel_s = frame.t_s - latch_t_s;
            if after_only {
                if rel_s <= 1e-9 {
                    continue;
                }
            } else if rel_s > 1e-9 {
                continue;
            }
            let mark = nearest_mark(markers, frame.t_s);
            let ev = ContactEvent::line(
                "ring",
                frame.sample,
                frame.t_s,
                LABELS[idx],
                frame.sites[idx],
                frame.loss_pct,
                frame.sr_hz,
                mark,
                Some(rel_s),
            );
            let _ = append_event(path, &ev);
        }
    }
}

pub fn sidecar_path(recording: &Path) -> PathBuf {
    let stem = recording.with_extension("");
    PathBuf::from(format!("{}.contact.jsonl", stem.display()))
}

fn append_event(recording: &Path, event: &ContactEvent) -> std::io::Result<()> {
    let path = sidecar_path(recording);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")?;
    Ok(())
}

fn is_recording_stopped_label(label: &str) -> bool {
    label.contains("Recording stopped")
}

fn recording_stopped_at(markers: &[MarkerEvent]) -> Option<f64> {
    markers
        .iter()
        .filter(|m| is_recording_stopped_label(&m.label))
        .map(|m| m.board_timestamp)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

fn after_recording_stopped(markers: &[MarkerEvent], t_s: f64) -> bool {
    recording_stopped_at(markers).is_some_and(|stop| t_s + 1e-9 >= stop)
}

fn nearest_mark(markers: &[MarkerEvent], t_s: f64) -> Option<String> {
    markers
        .iter()
        .filter(|m| (m.board_timestamp - t_s).abs() <= MARK_RANGE_SEC)
        .min_by(|a, b| {
            (a.board_timestamp - t_s)
                .abs()
                .partial_cmp(&(b.board_timestamp - t_s).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|m| m.label.clone())
}

/// Offline pass: walk a BDF and append latch/unlatch/ring lines beside it.
pub fn write_offline_sidecar(recording: &Path) -> std::io::Result<PathBuf> {
    let (rows, fs, n_exg, mut marks) = crate::data_writers::bdf::read_bdf(recording)?;
    marks.extend(crate::markers::load_sidecar(recording));
    marks.sort_by_key(|m| m.sample_index);
    marks.dedup_by(|a, b| a.sample_index == b.sample_index && a.label == b.label);

    let sr = fs.max(1) as f64;
    let win = (2.0 * sr).round() as usize;
    let n_exg = n_exg.min(8);
    let out = sidecar_path(recording);
    let _ = std::fs::remove_file(&out);

    let mut cols: Vec<Vec<f64>> = vec![Vec::with_capacity(rows.len()); n_exg];
    for row in &rows {
        for (col, c) in cols.iter_mut().enumerate() {
            c.push(row.get(col).copied().unwrap_or(0.0));
        }
    }

    let mut log = ContactLog::new();
    let mut chs = vec![Vec::with_capacity(win); n_exg];
    let start_at = win.max(2);
    let take_end_s = rows.len() as f64 / sr;
    let skip_tail = recording_stopped_at(&marks).is_some();
    for end in start_at..=rows.len() {
        let sample = end.saturating_sub(1);
        let t_s = sample as f64 / sr;
        // When the take has a Recording-stopped mark, skip the last 0.5 s (the stop click lives there).
        if skip_tail && t_s + 1e-9 >= take_end_s - STOP_TAIL_SEC {
            break;
        }
        let start = end - win;
        for (col, ch) in chs.iter_mut().enumerate() {
            ch.clear();
            ch.extend_from_slice(&cols[col][start..end]);
        }
        log.observe(&chs, sample, t_s, sr, 0.0, &marks, Some(recording));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::laterality::{channels_railed, contact_snapshot, RAIL_RATIO};
    use crate::markers::MarkerEvent;

    fn square_on_f7() -> Vec<Vec<f64>> {
        let mut chs = vec![vec![0.0; 250]; 8];
        for i in 0..8 {
            if i == 2 {
                continue;
            }
            chs[i] = (0..250).map(|t| 400.0 * ((t as f64) * 0.2).sin()).collect();
        }
        for t in 80..250 {
            chs[2][t] = 30_000.0;
        }
        chs
    }

    #[test]
    fn latch_event_is_square_step_vs_neighbors_not_dc() {
        let dir = std::env::temp_dir().join("openbci_contact_test");
        let _ = std::fs::create_dir_all(&dir);
        let rec = dir.join("synthetic.bdf");
        let side = sidecar_path(&rec);
        let _ = std::fs::remove_file(&side);

        let chs = square_on_f7();
        let snap = contact_snapshot(&chs);
        assert!(snap[2].max_step > 20_000.0, "max_step={}", snap[2].max_step);
        assert!(
            snap[2].neighbor_med < snap[2].max_step / 3.0,
            "med={} own={}",
            snap[2].neighbor_med,
            snap[2].max_step
        );
        assert!(snap[2].ratio > RAIL_RATIO, "ratio={}", snap[2].ratio);

        let mut log = ContactLog::new();
        log.observe(&chs, 249, 0.996, 250.0, 0.0, &[], Some(&rec));
        let body = std::fs::read_to_string(&side).expect("sidecar");
        let latch = body
            .lines()
            .find(|l| l.contains("\"event\":\"latch\""))
            .expect("latch line");
        let v: serde_json::Value = serde_json::from_str(latch).unwrap();
        assert_eq!(v["event"], "latch");
        assert_eq!(v["site"], "C3");
        assert_eq!(v["rule"], RULE);
        assert_ne!(v["rule"], "dc");
        assert_ne!(v["rule"], "p2p");
        assert!(v["max_step"].as_f64().unwrap() > 20_000.0);
        assert!(v["ratio"].as_f64().unwrap() > RAIL_RATIO);
        assert_eq!(v["loss_pct"], 0.0);
        assert_eq!(v["sr_hz"], 250.0);
        assert_eq!(v["rate_off"], false);
        assert_eq!(v["loss_up"], false);
        assert!(body.lines().any(|l| l.contains("\"event\":\"ring\"")));
        let _ = std::fs::remove_file(&side);
    }

    #[test]
    fn brow_raise_take_contact_event_and_sidecar() {
        let path =
            std::path::Path::new("Recordings/OpenBCI_2026-08-30_17-55-39_625_626066000_0.bdf");
        if !path.exists() {
            return;
        }
        let (rows, sr, n_exg, _) = crate::data_writers::bdf::read_bdf(path).unwrap();
        let sr = sr.max(1) as f64;
        let start = ((146.5 * sr) as usize).min(rows.len());
        let end = ((148.5 * sr) as usize).min(rows.len());
        let window = &rows[start..end];
        let mut chs = Vec::new();
        for col in 0..n_exg.min(8) {
            chs.push(
                window
                    .iter()
                    .map(|row| row.get(col).copied().unwrap_or(0.0))
                    .collect::<Vec<f64>>(),
            );
        }
        assert_eq!(
            channels_railed(&chs),
            [false, false, true, false, false, false, false, false],
            "ch2 square at 147.9s must be the only rail"
        );
        let snap = contact_snapshot(&chs);
        assert!(
            (snap[2].max_step - 31650.4).abs() < 500.0,
            "max_step={}",
            snap[2].max_step
        );
        assert!(
            snap[2].neighbor_med < 5000.0,
            "neighbor_med={}",
            snap[2].neighbor_med
        );
        assert!(
            snap[2].ratio > RAIL_RATIO,
            "ratio={} med={}",
            snap[2].ratio,
            snap[2].neighbor_med
        );

        let out = write_offline_sidecar(path).expect("write sidecar");
        let body = std::fs::read_to_string(&out).expect("read sidecar");
        let latch = body
            .lines()
            .find(|l| l.contains("\"event\":\"latch\"") && l.contains("\"site\":\"C3\""))
            .expect("C3 (ch2) latch line");
        let v: serde_json::Value = serde_json::from_str(latch).unwrap();
        assert_eq!(v["rule"], RULE);
        assert_eq!(v["site"], "C3");
        assert!(v["max_step"].as_f64().unwrap() > 20_000.0);
        assert!(v["ratio"].as_f64().unwrap() > RAIL_RATIO);
        assert!(v["neighbor_med"].as_f64().unwrap() < v["max_step"].as_f64().unwrap() / 3.0);
        assert_eq!(v["loss_pct"], 0.0);
        assert_eq!(v["rate_off"], false);
        assert_eq!(v["loss_up"], false);
        let mark = v["mark"].as_str().unwrap_or("");
        assert!(
            mark.contains("eyebrow") || mark.contains("7/10"),
            "mark={mark}"
        );
        let t_s = v["t_s"].as_f64().unwrap();
        assert!((t_s - 147.9).abs() < 0.1, "ch2 first latch t_s={t_s}");
        let latches: Vec<&str> = body
            .lines()
            .filter(|l| l.contains("\"event\":\"latch\""))
            .collect();
        assert!(
            latches.iter().all(|l| l.contains("\"site\":\"C3\"")),
            "latches must be C3/ch2-only: {latches:?}"
        );
        assert!(
            !body
                .lines()
                .any(|l| { l.contains("\"event\":\"latch\"") && l.contains("\"site\":\"P7\"") }),
            "P7 (ch4) must not latch at the Recording-stopped click"
        );
    }

    #[test]
    fn recording_stopped_click_does_not_latch() {
        let dir = std::env::temp_dir().join("openbci_contact_stop_click");
        let _ = std::fs::create_dir_all(&dir);
        let rec = dir.join("stop.bdf");
        let side = sidecar_path(&rec);
        let _ = std::fs::remove_file(&side);

        // High-ratio F7 square after mark 10 — still must not latch.
        let chs = square_on_f7();
        let markers = vec![MarkerEvent::new(200, 0.8, "10/10 Recording stopped.")];
        let mut log = ContactLog::new();
        log.observe(&chs, 249, 0.996, 250.0, 0.0, &markers, Some(&rec));
        let body = std::fs::read_to_string(&side).unwrap_or_default();
        assert!(
            !body.contains("\"event\":\"latch\""),
            "after Recording stopped the C3/F7 stop click must not latch: {body}"
        );
        let _ = std::fs::remove_file(&side);
    }
}
