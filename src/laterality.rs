//! Pair-named laterality: C3 vs C4 and O1 vs O2, per band.
//!
//! LI = (L − R) / (L + R) on one homologous pair. This is a power ratio on named
//! electrodes, not a hemisphere state and not a diagnosis.
//! Default Cyton 8ch (Ultracortex Mark IV docs): posterior pair is O1/O2.
//! Gamma (30–55 Hz) is EMG on 8ch and is not scored.

use crate::fft::{band_powers_psd, band_psd_excluding, mean_band_powers, EEG_BANDS};
use crate::filter_settings::NotchMode;

pub const IDX_FP1: usize = 0;
pub const IDX_FP2: usize = 1;
pub const IDX_C3: usize = 2;
pub const IDX_C4: usize = 3;
pub const IDX_P7: usize = 4;
pub const IDX_P8: usize = 5;
pub const IDX_O1: usize = 6;
pub const IDX_O2: usize = 7;

pub const LI_THRESHOLD: f64 = 0.15;
const POWER_FLOOR: f64 = 1e-8;
pub const WINDOW_SEC: f64 = 2.0;
pub const MUSCLE_LO_HZ: f64 = 30.0;
pub const MUSCLE_HI_CAP_HZ: f64 = 80.0;
const NOTCH_HOLE_LO: f64 = 59.0;
const NOTCH_HOLE_HI: f64 = 61.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pair {
    C3C4,
    O1O2,
}

impl Pair {
    pub const ALL: [Pair; 2] = [Pair::C3C4, Pair::O1O2];

    pub fn label(self) -> &'static str {
        match self {
            Pair::C3C4 => "C3/C4",
            Pair::O1O2 => "O1/O2",
        }
    }

    pub fn idx(self) -> usize {
        match self {
            Pair::C3C4 => 0,
            Pair::O1O2 => 1,
        }
    }

    pub fn left_idx(self) -> usize {
        match self {
            Pair::C3C4 => IDX_C3,
            Pair::O1O2 => IDX_O1,
        }
    }

    pub fn right_idx(self) -> usize {
        match self {
            Pair::C3C4 => IDX_C4,
            Pair::O1O2 => IDX_O2,
        }
    }

    pub fn electrode(self, side: Side) -> &'static str {
        match (self, side) {
            (Pair::C3C4, Side::Left) => "C3",
            (Pair::C3C4, Side::Right) => "C4",
            (Pair::O1O2, Side::Left) => "O1",
            (Pair::O1O2, Side::Right) => "O2",
        }
    }
}

/// Scored rhythms only. Gamma is EMG and is not a laterality row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rhythm {
    Delta,
    Theta,
    Alpha,
    Beta,
}

impl Rhythm {
    pub const SCORED: [Rhythm; 4] = [Rhythm::Delta, Rhythm::Theta, Rhythm::Alpha, Rhythm::Beta];

    pub fn psd_index(self) -> usize {
        match self {
            Rhythm::Delta => 0,
            Rhythm::Theta => 1,
            Rhythm::Alpha => 2,
            Rhythm::Beta => 3,
        }
    }

    pub fn greek(self) -> &'static str {
        match self {
            Rhythm::Delta => "δ",
            Rhythm::Theta => "θ",
            Rhythm::Alpha => "α",
            Rhythm::Beta => "β",
        }
    }

    pub fn name(self) -> &'static str {
        EEG_BANDS[self.psd_index()].0
    }

    pub fn hz(self) -> (f64, f64) {
        let (_, lo, hi) = EEG_BANDS[self.psd_index()];
        (lo, hi)
    }

    pub fn hold_sec(self) -> f64 {
        match self {
            Rhythm::Delta => 3.0,
            Rhythm::Theta => 2.0,
            Rhythm::Alpha => 1.5,
            Rhythm::Beta => 1.0,
        }
    }

    /// max(band hold, 2.5 cycles of band center). A 200 ms blip cannot qualify.
    pub fn flip_hold_sec(self) -> f64 {
        let (lo, hi) = self.hz();
        let center = 0.5 * (lo + hi);
        let cycles = if center > 1e-9 {
            2.5 / center
        } else {
            self.hold_sec()
        };
        self.hold_sec().max(cycles)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PairBandLi {
    pub pair: Pair,
    pub rhythm: Rhythm,
    pub left_psd: f64,
    pub right_psd: f64,
    pub li: Option<f64>,
}

impl PairBandLi {
    pub fn side(&self) -> Option<Side> {
        match self.li {
            Some(v) if v >= LI_THRESHOLD => Some(Side::Left),
            Some(v) if v <= -LI_THRESHOLD => Some(Side::Right),
            _ => None,
        }
    }

    /// ERD: for C3/C4 α only, the drop is the electrode with *less* α power.
    pub fn alpha_drop_copy(&self) -> Option<String> {
        if self.pair != Pair::C3C4 || self.rhythm != Rhythm::Alpha {
            return None;
        }
        match self.side() {
            Some(Side::Left) => Some("α drop on C4".into()),
            Some(Side::Right) => Some("α drop on C3".into()),
            None => None,
        }
    }

    pub fn row_copy(&self) -> String {
        let pair = self.pair.label();
        let g = self.rhythm.greek();
        match self.li {
            None => format!("{pair} {g}  —"),
            Some(li) => {
                let mut s = format!("{pair} {g}  LI {li:+.2}");
                if let Some(drop) = self.alpha_drop_copy() {
                    s.push_str(" · ");
                    s.push_str(&drop);
                }
                s
            }
        }
    }
}

/// LI on each scored pair×band. Gamma is omitted.
pub fn pair_band_snapshot(channels: &[Vec<f64>], sr: f64) -> Vec<PairBandLi> {
    let mut out = Vec::with_capacity(8);
    for pair in Pair::ALL {
        let left = channels.get(pair.left_idx()).cloned().unwrap_or_default();
        let right = channels.get(pair.right_idx()).cloned().unwrap_or_default();
        let lp = if left.is_empty() {
            [0.0; 5]
        } else {
            band_powers_psd(&left, sr)
        };
        let rp = if right.is_empty() {
            [0.0; 5]
        } else {
            band_powers_psd(&right, sr)
        };
        for rhythm in Rhythm::SCORED {
            let i = rhythm.psd_index();
            let l = lp[i];
            let r = rp[i];
            let li = laterality_index(l, r);
            out.push(PairBandLi {
                pair,
                rhythm,
                left_psd: l,
                right_psd: r,
                li,
            });
        }
    }
    out
}

/// Per-channel PSD in one scored rhythm. Index 0..7 is the 8ch montage
/// (Fp1, Fp2, C3, C4, P7, P8, O1, O2) — not only the overlay pair.
pub fn channel_band_psd(channels: &[Vec<f64>], sr: f64, rhythm: Rhythm) -> [f64; 8] {
    let mut out = [0.0; 8];
    let i = rhythm.psd_index();
    for (ch, samples) in channels.iter().enumerate().take(8) {
        if samples.is_empty() {
            continue;
        }
        out[ch] = band_powers_psd(samples, sr)[i];
    }
    out
}

/// Disc fill 0..1 from that channel's power in the overlay band.
/// Alive channels floor at 0.7 so a quiet insert still paints a peach disc
/// (same floor the pair used to get); the loudest is 1.0. Below [`POWER_FLOOR`]
/// stays 0 (hairline / empty of activity).
pub fn occupied_band_fill(psd: &[f64; 8]) -> [f32; 8] {
    let maxp = psd.iter().copied().fold(1e-12_f64, f64::max);
    let mut out = [0.0_f32; 8];
    for i in 0..8 {
        if psd[i] > POWER_FLOOR {
            out[i] = (0.7 + 0.3 * (psd[i] / maxp)).clamp(0.0, 1.0) as f32;
        }
    }
    out
}

pub fn laterality_index(left: f64, right: f64) -> Option<f64> {
    let s = left + right;
    if s <= POWER_FLOOR {
        return None;
    }
    Some(((left - right) / s).clamp(-1.0, 1.0))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveRow {
    pub rhythm: Rhythm,
    pub rel: f64,
    pub probable: bool,
}

impl WaveRow {
    pub fn row_copy(&self) -> String {
        let pct = (self.rel * 100.0).clamp(0.0, 100.0);
        let (lo, hi) = self.rhythm.hz();
        let tag = if self.probable {
            "probable"
        } else {
            "not dominant"
        };
        format!(
            "{} {:.0}–{:.0} Hz  {:.0}% of 1–30 Hz · {tag} · conf {:.0}%",
            self.rhythm.name(),
            lo,
            hi,
            pct,
            pct
        )
    }
}

/// Sudden square step that dwells, not DC offset and not a wide peak-to-peak.
/// F7 contact loss on the 17:55 take is ~30 kµV; muscle consecutive diffs stay well below this.
const SQUARE_STEP_UV: f64 = 8000.0;
/// ~64 ms at 250 Hz. A spike that returns fails this; a rail that stays passes.
const SQUARE_DWELL: usize = 16;
/// Own max-step vs neighbor median. F7 contact loss is ~20×; a whole-board stop click is ~3× and must not rail.
pub const RAIL_RATIO: f64 = 10.0;

fn max_abs_step(samples: &[f64]) -> f64 {
    samples
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max)
}

fn count_big_steps(samples: &[f64]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[1] - w[0]).abs() >= SQUARE_STEP_UV)
        .count()
}

fn has_square_step(samples: &[f64]) -> bool {
    if samples.len() < 3 {
        return false;
    }
    // F7 contact chatter: many 30 kµV squares, never dwells.
    if count_big_steps(samples) >= 3 {
        return true;
    }
    if samples.len() < SQUARE_DWELL + 2 {
        return false;
    }
    let mut best_i = 0usize;
    let mut best = 0.0;
    for i in 0..samples.len() - 1 {
        let d = (samples[i + 1] - samples[i]).abs();
        if d > best {
            best = d;
            best_i = i;
        }
    }
    if best < SQUARE_STEP_UV {
        return false;
    }
    let old = samples[best_i];
    let newv = samples[best_i + 1];
    let start = best_i + 1;
    let end = (start + SQUARE_DWELL).min(samples.len());
    if end - start < SQUARE_DWELL {
        return false;
    }
    let dwell = samples[start..end]
        .iter()
        .filter(|&&x| (x - newv).abs() <= (x - old).abs())
        .count();
    dwell >= SQUARE_DWELL
}

pub fn channel_railed(samples: &[f64]) -> bool {
    has_square_step(samples)
}

/// Square step *and* out of line with neighbors. A common-mode pop does not paint the plate.
pub fn channels_railed(channels: &[Vec<f64>]) -> [bool; 8] {
    let mut out = [false; 8];
    let n = channels.len().min(8);
    if n == 0 {
        return out;
    }
    let steps: Vec<f64> = (0..n).map(|i| max_abs_step(&channels[i])).collect();
    for i in 0..n {
        if !has_square_step(&channels[i]) {
            continue;
        }
        let own = steps[i];
        let mut others: Vec<f64> = steps
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, v)| *v)
            .collect();
        others.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = if others.is_empty() {
            0.0
        } else {
            others[others.len() / 2]
        };
        if own > med * RAIL_RATIO {
            out[i] = true;
        }
    }
    out
}

pub fn latch_rails(current: &mut [bool; 8], channels: &[Vec<f64>]) {
    let flags = channels_railed(channels);
    for i in 0..8 {
        if flags[i] {
            current[i] = true;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SiteContact {
    pub max_step: f64,
    pub neighbor_med: f64,
    pub ratio: f64,
    pub p2p: f64,
}

fn peak_to_peak(samples: &[f64]) -> f64 {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &x in samples {
        if x < min {
            min = x;
        }
        if x > max {
            max = x;
        }
    }
    if min.is_finite() && max.is_finite() {
        max - min
    } else {
        0.0
    }
}

/// Per-site square-step stats for the contact sidecar. Does not decide the latch.
pub fn contact_snapshot(channels: &[Vec<f64>]) -> [SiteContact; 8] {
    let n = channels.len().min(8);
    let mut steps = [0.0_f64; 8];
    for i in 0..n {
        steps[i] = max_abs_step(&channels[i]);
    }
    let mut out = [SiteContact::default(); 8];
    for i in 0..n {
        let own = steps[i];
        let mut others: Vec<f64> = (0..n).filter(|&j| j != i).map(|j| steps[j]).collect();
        others.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = if others.is_empty() {
            0.0
        } else {
            others[others.len() / 2]
        };
        let ratio = if med > 0.0 {
            own / med
        } else if own > 0.0 {
            1e9
        } else {
            0.0
        };
        out[i] = SiteContact {
            max_step: own,
            neighbor_med: med,
            ratio,
            p2p: peak_to_peak(&channels[i]),
        };
    }
    out
}

/// Relative spectral power on these electrodes (Δθ αβ), not a clinical scorer.
pub fn wave_presence(channels: &[Vec<f64>], sr: f64) -> [WaveRow; 4] {
    let p = mean_band_powers(channels, sr);
    let denom = p[0] + p[1] + p[2] + p[3];
    let mut rels = [0.0; 4];
    if denom > POWER_FLOOR {
        for i in 0..4 {
            rels[i] = (p[i] / denom).clamp(0.0, 1.0);
        }
    }
    let max = rels.iter().copied().fold(0.0_f64, f64::max);
    let n_max = rels.iter().filter(|&&r| (r - max).abs() < 1e-12).count();
    let mut out = [WaveRow {
        rhythm: Rhythm::Delta,
        rel: 0.0,
        probable: false,
    }; 4];
    for (i, rhythm) in Rhythm::SCORED.iter().enumerate() {
        let rel = rels[i];
        let probable = rel >= 0.30 && n_max == 1 && (rel - max).abs() < 1e-12;
        out[i] = WaveRow {
            rhythm: *rhythm,
            rel,
            probable,
        };
    }
    out
}

/// 30 Hz → min(high_cut, 80). None when the bar high is ≤ 30.
pub fn muscle_band(high_cut: f64) -> Option<(f64, f64)> {
    if high_cut <= MUSCLE_LO_HZ {
        None
    } else {
        Some((MUSCLE_LO_HZ, high_cut.min(MUSCLE_HI_CAP_HZ)))
    }
}

pub fn muscle_label(high_cut: f64, notch: NotchMode) -> String {
    match muscle_band(high_cut) {
        None => "30 Hz+ (often muscle) — off (high ≤ 30)".into(),
        Some((lo, hi)) => {
            let mut s = format!("{lo:.0}–{hi:.0} Hz (often muscle)");
            if notch_hole_at_60(notch, lo, hi) {
                s.push_str(" · notch hole at 60");
            }
            s
        }
    }
}

fn notch_hole_at_60(notch: NotchMode, lo: f64, hi: f64) -> bool {
    notch.punches_60() && lo < 60.0 && hi > 60.0
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContaminationMeter {
    pub label: String,
    pub ratio: f64,
    pub frontal_ratio: f64,
    pub active: bool,
}

impl ContaminationMeter {
    pub fn row_copy(&self) -> String {
        if !self.active {
            return self.label.clone();
        }
        format!(
            "{}  {:.0}% of 1–30 Hz · Fp1/Fp2 {:.0}%",
            self.label,
            self.ratio * 100.0,
            self.frontal_ratio * 100.0
        )
    }
}

fn mean_psd_excluding(
    channels: &[Vec<f64>],
    sr: f64,
    lo: f64,
    hi: f64,
    exclude: Option<(f64, f64)>,
) -> f64 {
    let mut acc = 0.0;
    let mut n = 0usize;
    for ch in channels {
        if ch.is_empty() {
            continue;
        }
        acc += band_psd_excluding(ch, sr, lo, hi, exclude);
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        acc / n as f64
    }
}

fn mean_psd_idxs(
    channels: &[Vec<f64>],
    idxs: &[usize],
    sr: f64,
    lo: f64,
    hi: f64,
    exclude: Option<(f64, f64)>,
) -> f64 {
    let mut acc = 0.0;
    let mut n = 0usize;
    for &i in idxs {
        if let Some(ch) = channels.get(i) {
            if ch.is_empty() {
                continue;
            }
            acc += band_psd_excluding(ch, sr, lo, hi, exclude);
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        acc / n as f64
    }
}

/// 30–min(high, 80) vs 1–30 Hz. Scoring only — no extra Butterworth. Does not drive flips.
pub fn contamination_meter(
    channels: &[Vec<f64>],
    sr: f64,
    high_cut: f64,
    notch: NotchMode,
) -> ContaminationMeter {
    let label = muscle_label(high_cut, notch);
    let Some((lo, hi)) = muscle_band(high_cut) else {
        return ContaminationMeter {
            label,
            ratio: 0.0,
            frontal_ratio: 0.0,
            active: false,
        };
    };
    let exclude = if notch_hole_at_60(notch, lo, hi) {
        Some((NOTCH_HOLE_LO, NOTCH_HOLE_HI))
    } else {
        None
    };
    let muscle = mean_psd_excluding(channels, sr, lo, hi, exclude);
    let base = mean_psd_excluding(channels, sr, 1.0, 30.0, None);
    let ratio = if base > POWER_FLOOR {
        muscle / base
    } else if muscle > POWER_FLOOR {
        1.0
    } else {
        0.0
    };
    let f_muscle = mean_psd_idxs(channels, &[IDX_FP1, IDX_FP2], sr, lo, hi, exclude);
    let f_base = mean_psd_idxs(channels, &[IDX_FP1, IDX_FP2], sr, 1.0, 30.0, None);
    let frontal_ratio = if f_base > POWER_FLOOR {
        f_muscle / f_base
    } else if f_muscle > POWER_FLOOR {
        1.0
    } else {
        0.0
    };
    ContaminationMeter {
        label,
        ratio,
        frontal_ratio,
        active: true,
    }
}

#[derive(Clone, Debug)]
struct Cell {
    confirmed: Option<Side>,
    candidate: Option<Side>,
    candidate_since: f64,
    flip_count: u32,
}

impl Cell {
    fn new() -> Self {
        Self {
            confirmed: None,
            candidate: None,
            candidate_since: 0.0,
            flip_count: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlipEvent {
    pub pair: Pair,
    pub rhythm: Rhythm,
    pub from: Side,
    pub to: Side,
    pub at_sec: f64,
}

impl FlipEvent {
    pub fn line(&self, now_sec: f64) -> String {
        let ago = (now_sec - self.at_sec).max(0.0);
        let pair = self.pair.label();
        let g = self.rhythm.greek();
        let from = self.pair.electrode(self.from);
        let to = self.pair.electrode(self.to);
        if self.rhythm == Rhythm::Alpha && self.pair == Pair::C3C4 {
            format!("{pair} {g}  {from}→{to} drop {ago:.1} s ago")
        } else {
            format!("{pair} {g}  {from}→{to} {ago:.1} s ago")
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlipClock {
    cells: [[Cell; 4]; 2],
    last: Option<FlipEvent>,
}

impl Default for FlipClock {
    fn default() -> Self {
        Self::new()
    }
}

impl FlipClock {
    pub fn new() -> Self {
        Self {
            cells: [
                [Cell::new(), Cell::new(), Cell::new(), Cell::new()],
                [Cell::new(), Cell::new(), Cell::new(), Cell::new()],
            ],
            last: None,
        }
    }

    pub fn observe_rows(&mut self, rows: &[PairBandLi], now_sec: f64) {
        for row in rows {
            self.observe(row.pair, row.rhythm, row.side(), now_sec);
        }
    }

    pub fn observe(&mut self, pair: Pair, rhythm: Rhythm, side: Option<Side>, now_sec: f64) {
        let hold = rhythm.flip_hold_sec();
        let cell = &mut self.cells[pair.idx()][rhythm.psd_index()];
        match side {
            None => {
                cell.candidate = None;
            }
            Some(s) if cell.confirmed == Some(s) => {
                cell.candidate = None;
            }
            Some(s) => {
                if cell.candidate != Some(s) {
                    cell.candidate = Some(s);
                    cell.candidate_since = now_sec;
                }
                if now_sec - cell.candidate_since + 1e-9 >= hold {
                    if let Some(prev) = cell.confirmed {
                        if prev != s {
                            let (from, to) = if rhythm == Rhythm::Alpha && pair == Pair::C3C4 {
                                (prev.opposite(), s.opposite())
                            } else {
                                (prev, s)
                            };
                            self.last = Some(FlipEvent {
                                pair,
                                rhythm,
                                from,
                                to,
                                at_sec: now_sec,
                            });
                            cell.flip_count += 1;
                        }
                    }
                    cell.confirmed = Some(s);
                    cell.candidate = None;
                }
            }
        }
    }

    pub fn flip_count(&self, pair: Pair, rhythm: Rhythm) -> u32 {
        self.cells[pair.idx()][rhythm.psd_index()].flip_count
    }

    pub fn total_flips(&self) -> u32 {
        self.cells
            .iter()
            .flat_map(|row| row.iter())
            .map(|c| c.flip_count)
            .sum()
    }

    pub fn last_flip_line(&self, now_sec: f64) -> String {
        match &self.last {
            None => "no flip yet".into(),
            Some(ev) => format!(
                "Last flip: {} · {} flips this session",
                ev.line(now_sec),
                self.total_flips()
            ),
        }
    }
}

pub struct SelfTestReport {
    pub passed: usize,
    pub failed: usize,
    pub lines: Vec<String>,
}

impl SelfTestReport {
    pub fn ok(&self) -> bool {
        self.failed == 0 && self.passed > 0
    }
}

pub fn run_self_test() -> SelfTestReport {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut lines = Vec::new();
    for (name, ok, detail) in fixture_checks() {
        if ok {
            passed += 1;
            lines.push(format!("PASS  {name}  {detail}"));
        } else {
            failed += 1;
            lines.push(format!("FAIL  {name}  {detail}"));
        }
    }
    SelfTestReport {
        passed,
        failed,
        lines,
    }
}

fn fixture_checks() -> Vec<(&'static str, bool, String)> {
    let sr = 250.0;
    let n = 512usize;
    let c3_alpha = eight_ch_tone(n, sr, IDX_C3, 10.0);
    let rows = pair_band_snapshot(&c3_alpha, sr);
    let alpha_c3c4 = rows
        .iter()
        .find(|r| r.pair == Pair::C3C4 && r.rhythm == Rhythm::Alpha)
        .cloned();
    let copy = alpha_c3c4
        .as_ref()
        .map(|r| r.row_copy())
        .unwrap_or_default();

    let equal = eight_ch_equal_tone(n, sr, IDX_C3, IDX_C4, 10.0);
    let eq_rows = pair_band_snapshot(&equal, sr);
    let eq_a = eq_rows
        .iter()
        .find(|r| r.pair == Pair::C3C4 && r.rhythm == Rhythm::Alpha)
        .cloned();

    let mut clock_ok = FlipClock::new();
    hold_side(
        &mut clock_ok,
        Pair::C3C4,
        Rhythm::Delta,
        Some(Side::Left),
        0.0,
        3.0,
    );
    hold_side(
        &mut clock_ok,
        Pair::C3C4,
        Rhythm::Delta,
        Some(Side::Right),
        3.0,
        3.0,
    );

    let mut clock_blip = FlipClock::new();
    hold_side(
        &mut clock_blip,
        Pair::C3C4,
        Rhythm::Delta,
        Some(Side::Left),
        0.0,
        3.0,
    );
    hold_side(
        &mut clock_blip,
        Pair::C3C4,
        Rhythm::Delta,
        Some(Side::Right),
        3.0,
        0.2,
    );
    hold_side(
        &mut clock_blip,
        Pair::C3C4,
        Rhythm::Delta,
        Some(Side::Left),
        3.2,
        3.0,
    );

    let mut clock_b = FlipClock::new();
    hold_side(
        &mut clock_b,
        Pair::C3C4,
        Rhythm::Beta,
        Some(Side::Left),
        0.0,
        1.0,
    );
    hold_side(
        &mut clock_b,
        Pair::C3C4,
        Rhythm::Beta,
        Some(Side::Right),
        1.0,
        1.0,
    );

    let mut clock_b_short = FlipClock::new();
    hold_side(
        &mut clock_b_short,
        Pair::C3C4,
        Rhythm::Beta,
        Some(Side::Left),
        0.0,
        1.0,
    );
    hold_side(
        &mut clock_b_short,
        Pair::C3C4,
        Rhythm::Beta,
        Some(Side::Right),
        1.0,
        0.3,
    );

    let waves = wave_presence(&eight_ch_tone(n, sr, IDX_C3, 10.0), sr);

    vec![
        (
            "C3-only α",
            alpha_c3c4
                .as_ref()
                .is_some_and(|r| r.li.is_some_and(|v| v > 0.0))
                && copy.contains("C3/C4")
                && copy.contains("α drop on C4"),
            copy,
        ),
        (
            "equal power",
            eq_a.as_ref().is_some_and(|r| r.side().is_none()),
            eq_a.as_ref().map(|r| r.row_copy()).unwrap_or_default(),
        ),
        (
            "δ 3 s flip",
            clock_ok.flip_count(Pair::C3C4, Rhythm::Delta) == 1,
            clock_ok.last_flip_line(6.0),
        ),
        (
            "δ 200 ms blip",
            clock_blip.flip_count(Pair::C3C4, Rhythm::Delta) == 0,
            clock_blip.last_flip_line(6.2),
        ),
        (
            "β 1 s hold",
            clock_b.flip_count(Pair::C3C4, Rhythm::Beta) == 1,
            clock_b.last_flip_line(2.0),
        ),
        (
            "β 0.3 s no flip",
            clock_b_short.flip_count(Pair::C3C4, Rhythm::Beta) == 0,
            clock_b_short.last_flip_line(1.3),
        ),
        (
            "no γ laterality",
            rows.iter().all(|r| r.rhythm != Rhythm::Alpha || true)
                && !rows
                    .iter()
                    .any(|r| r.row_copy().to_lowercase().contains("gamma"))
                && rows.len() == 8,
            format!("{} rows", rows.len()),
        ),
        (
            "10 Hz probable α",
            waves[2].probable
                && waves[2].rhythm == Rhythm::Alpha
                && !waves[0].probable
                && !waves[1].probable
                && !waves[3].probable,
            waves[2].row_copy(),
        ),
    ]
}

fn hold_side(
    clock: &mut FlipClock,
    pair: Pair,
    rhythm: Rhythm,
    side: Option<Side>,
    t0: f64,
    dur: f64,
) {
    let dt = 0.05;
    let n = ((dur / dt).round() as i32).max(1);
    for i in 0..=n {
        let t = t0 + i as f64 * dt;
        if t > t0 + dur + 1e-9 {
            break;
        }
        clock.observe(pair, rhythm, side, t);
    }
}

fn sine(n: usize, sr: f64, hz: f64, amp: f64) -> Vec<f64> {
    (0..n)
        .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / sr).sin())
        .collect()
}

fn eight_ch_tone(n: usize, sr: f64, ch: usize, hz: f64) -> Vec<Vec<f64>> {
    let mut chs = vec![vec![0.0; n]; 8];
    chs[ch] = sine(n, sr, hz, 10.0);
    chs
}

fn eight_ch_equal_tone(n: usize, sr: f64, a: usize, b: usize, hz: f64) -> Vec<Vec<f64>> {
    let mut chs = vec![vec![0.0; n]; 8];
    let s = sine(n, sr, hz, 10.0);
    chs[a] = s.clone();
    chs[b] = s;
    chs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_jumps_count_as_railed() {
        let mut sq = vec![0.0; 250];
        for i in 80..250 {
            sq[i] = 30_000.0;
        }
        assert!(channel_railed(&sq));
        assert!(!channel_railed(&[1.0; 250]));
    }

    #[test]
    fn millivolt_dc_is_not_railed() {
        assert!(!channel_railed(&[20000.0; 250]));
    }

    #[test]
    fn wide_peak_to_peak_sine_is_not_railed() {
        let s: Vec<f64> = (0..250).map(|i| 5000.0 * (i as f64 * 0.1).sin()).collect();
        assert!(!channel_railed(&s));
    }

    #[test]
    fn spike_that_returns_is_not_railed() {
        let mut s = vec![0.0; 250];
        s[80] = 30_000.0;
        assert!(!channel_railed(&s));
    }

    #[test]
    fn only_square_channel_vs_neighbors() {
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
        let f = channels_railed(&chs);
        assert_eq!(f, [false, false, true, false, false, false, false, false]);
    }

    #[test]
    fn common_mode_square_does_not_paint_the_plate() {
        let mut chs = vec![vec![0.0; 250]; 8];
        for c in &mut chs {
            for t in 100..250 {
                c[t] = 30_000.0;
            }
        }
        let f = channels_railed(&chs);
        assert!(f.iter().all(|&x| !x), "{f:?}");
    }

    #[test]
    fn whole_board_stop_jump_does_not_rail() {
        // All channels ~11 kµV step; C3 a bit higher so ratio ~3. Same shape as the 17:55 stop click.
        let mut chs = vec![vec![0.0; 250]; 8];
        for c in 0..8 {
            let amp = if c == IDX_C3 { 33_000.0 } else { 11_000.0 };
            for t in 100..250 {
                chs[c][t] = amp;
            }
        }
        let snap = contact_snapshot(&chs);
        let ratio = snap[IDX_C3].ratio;
        assert!(
            ratio > 2.5 && ratio < 4.0,
            "fixture ratio should be ~3, got {ratio}"
        );
        assert!(
            ratio < RAIL_RATIO,
            "ratio {ratio} must fail the gate (RAIL_RATIO={RAIL_RATIO})"
        );
        let f = channels_railed(&chs);
        assert!(f.iter().all(|&x| !x), "no site should rail, got {f:?}");
    }

    #[test]
    fn brow_raise_take_only_f7_railed_after_jump() {
        let path =
            std::path::Path::new("Recordings/OpenBCI_2026-08-30_17-55-39_625_626066000_0.bdf");
        if !path.exists() {
            return;
        }
        let (rows, sr, n_exg, _) = crate::data_writers::bdf::read_bdf(path).unwrap();
        let sr = sr.max(1) as f64;
        let end = ((148.5 * sr) as usize).min(rows.len());
        let start = end.saturating_sub((2.0 * sr) as usize);
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
            "F7 (ch2) square at 147.9s must be the only rail"
        );
    }

    #[test]
    fn c3_only_alpha_li_positive_drop_on_c4() {
        let chs = eight_ch_tone(512, 250.0, IDX_C3, 10.0);
        let rows = pair_band_snapshot(&chs, 250.0);
        let r = rows
            .iter()
            .find(|r| r.pair == Pair::C3C4 && r.rhythm == Rhythm::Alpha)
            .expect("C3/C4 α row");
        assert!(r.li.unwrap() > 0.0, "LI={}", r.li.unwrap());
        assert_eq!(r.side(), Some(Side::Left));
        assert_eq!(r.alpha_drop_copy().as_deref(), Some("α drop on C4"));
        let copy = r.row_copy();
        assert!(copy.contains("C3/C4"), "{copy}");
        assert!(copy.contains("α drop on C4"), "{copy}");
    }

    #[test]
    fn equal_power_has_no_side() {
        let chs = eight_ch_equal_tone(512, 250.0, IDX_C3, IDX_C4, 10.0);
        let rows = pair_band_snapshot(&chs, 250.0);
        let r = rows
            .iter()
            .find(|r| r.pair == Pair::C3C4 && r.rhythm == Rhythm::Alpha)
            .unwrap();
        assert!(r.side().is_none(), "LI={:?}", r.li);
        let mut clock = FlipClock::new();
        clock.observe_rows(&rows, 0.0);
        clock.observe_rows(&rows, 5.0);
        assert_eq!(clock.total_flips(), 0);
    }

    #[test]
    fn delta_three_second_hold_is_one_flip_blip_is_not() {
        let mut clock = FlipClock::new();
        hold_side(
            &mut clock,
            Pair::C3C4,
            Rhythm::Delta,
            Some(Side::Left),
            0.0,
            3.0,
        );
        hold_side(
            &mut clock,
            Pair::C3C4,
            Rhythm::Delta,
            Some(Side::Right),
            3.0,
            3.0,
        );
        assert_eq!(clock.flip_count(Pair::C3C4, Rhythm::Delta), 1);
        let line = clock.last_flip_line(6.0);
        assert!(line.contains("C3/C4"), "{line}");
        assert!(line.contains('δ'), "{line}");

        let mut blip = FlipClock::new();
        hold_side(
            &mut blip,
            Pair::C3C4,
            Rhythm::Delta,
            Some(Side::Left),
            0.0,
            3.0,
        );
        hold_side(
            &mut blip,
            Pair::C3C4,
            Rhythm::Delta,
            Some(Side::Right),
            3.0,
            0.2,
        );
        hold_side(
            &mut blip,
            Pair::C3C4,
            Rhythm::Delta,
            Some(Side::Left),
            3.2,
            3.0,
        );
        assert_eq!(blip.flip_count(Pair::C3C4, Rhythm::Delta), 0);
    }

    #[test]
    fn beta_one_second_hold_flips_point_three_does_not() {
        let mut ok = FlipClock::new();
        hold_side(
            &mut ok,
            Pair::C3C4,
            Rhythm::Beta,
            Some(Side::Left),
            0.0,
            1.0,
        );
        hold_side(
            &mut ok,
            Pair::C3C4,
            Rhythm::Beta,
            Some(Side::Right),
            1.0,
            1.0,
        );
        assert_eq!(ok.flip_count(Pair::C3C4, Rhythm::Beta), 1);

        let mut short = FlipClock::new();
        hold_side(
            &mut short,
            Pair::C3C4,
            Rhythm::Beta,
            Some(Side::Left),
            0.0,
            1.0,
        );
        hold_side(
            &mut short,
            Pair::C3C4,
            Rhythm::Beta,
            Some(Side::Right),
            1.0,
            0.3,
        );
        assert_eq!(short.flip_count(Pair::C3C4, Rhythm::Beta), 0);
    }

    #[test]
    fn gamma_has_no_laterality_row() {
        let chs = eight_ch_tone(512, 250.0, IDX_C3, 10.0);
        let rows = pair_band_snapshot(&chs, 250.0);
        assert_eq!(rows.len(), 8);
        assert!(rows.iter().all(|r| matches!(
            r.rhythm,
            Rhythm::Delta | Rhythm::Theta | Rhythm::Alpha | Rhythm::Beta
        )));
        for r in &rows {
            let c = r.row_copy();
            assert!(!c.to_lowercase().contains("gamma"), "{c}");
            assert!(!c.contains('γ'), "{c}");
        }
    }

    #[test]
    fn muscle_copy_follows_high_cut() {
        assert_eq!(
            muscle_label(40.0, NotchMode::Off),
            "30–40 Hz (often muscle)"
        );
        assert_eq!(
            muscle_label(50.0, NotchMode::Off),
            "30–50 Hz (often muscle)"
        );
        assert_eq!(
            muscle_label(80.0, NotchMode::Off),
            "30–80 Hz (often muscle)"
        );
        assert_eq!(
            muscle_label(200.0, NotchMode::Off),
            "30–80 Hz (often muscle)"
        );
        let with_hole = muscle_label(80.0, NotchMode::Sixty);
        assert!(with_hole.starts_with("30–80 Hz (often muscle)"));
        assert!(with_hole.contains("notch hole at 60"), "{with_hole}");
        assert!(!muscle_label(50.0, NotchMode::Sixty).contains("notch hole"));
    }

    #[test]
    fn thirty_five_hz_on_f7_f8_raises_contamination_when_high_is_40() {
        let sr = 250.0;
        let n = 512usize;
        let mut chs = vec![vec![0.0; n]; 8];
        chs[IDX_FP1] = sine(n, sr, 35.0, 10.0);
        chs[IDX_FP2] = sine(n, sr, 35.0, 10.0);
        let m = contamination_meter(&chs, sr, 40.0, NotchMode::Off);
        assert!(m.active);
        assert!(m.label.starts_with("30–40 Hz"), "{}", m.label);
        assert!(
            m.ratio > 1.0,
            "35 Hz vs 1–30 Hz should raise the meter, ratio={}",
            m.ratio
        );
        assert!(m.frontal_ratio > 1.0, "Fp1/Fp2 ratio={}", m.frontal_ratio);
        let mut clock = FlipClock::new();
        clock.observe_rows(&pair_band_snapshot(&chs, sr), 0.0);
        clock.observe_rows(&pair_band_snapshot(&chs, sr), 5.0);
        assert_eq!(clock.total_flips(), 0);
    }

    #[test]
    fn ten_hz_alpha_keeps_contamination_low() {
        let chs = eight_ch_tone(512, 250.0, IDX_C3, 10.0);
        let m = contamination_meter(&chs, 250.0, 50.0, NotchMode::Off);
        assert!(
            m.ratio < 0.25,
            "10 Hz should not fill 30–50 Hz, ratio={}",
            m.ratio
        );
    }

    #[test]
    fn ten_hz_is_probable_alpha() {
        let chs = eight_ch_tone(512, 250.0, IDX_C3, 10.0);
        let w = wave_presence(&chs, 250.0);
        assert!(w[2].probable, "{}", w[2].row_copy());
        assert_eq!(w[2].rhythm, Rhythm::Alpha);
        assert!(!w[0].probable);
        assert!(!w[1].probable);
        assert!(!w[3].probable);
        let copy = w[2].row_copy();
        assert!(copy.contains("Alpha"), "{copy}");
        assert!(copy.contains("probable"), "{copy}");
        assert!(
            copy.contains("1–30 Hz") || copy.contains("1-30 Hz"),
            "{copy}"
        );
    }

    #[test]
    fn posterior_pair_is_o1_o2() {
        assert_eq!(Pair::O1O2.label(), "O1/O2");
        let chs = eight_ch_tone(512, 250.0, IDX_O1, 3.0);
        let rows = pair_band_snapshot(&chs, 250.0);
        let r = rows
            .iter()
            .find(|r| r.pair == Pair::O1O2 && r.rhythm == Rhythm::Delta)
            .unwrap();
        assert!(r.row_copy().contains("O1/O2"));
        assert!(!r.row_copy().contains("P3"));
    }

    #[test]
    fn self_test_fixtures_pass() {
        let report = run_self_test();
        assert!(
            report.ok(),
            "self-test failed:\n{}",
            report.lines.join("\n")
        );
    }

    #[test]
    fn insufficient_power_is_none() {
        assert_eq!(laterality_index(0.0, 0.0), None);
        assert!(laterality_index(1.0, 0.0).unwrap() > 0.9);
    }

    #[test]
    fn occupied_fill_uses_per_channel_band_not_only_the_overlay_pair() {
        let sr = 250.0;
        let n = 512usize;
        let chs = eight_ch_tone(n, sr, IDX_FP1, 10.0);
        let psd = channel_band_psd(&chs, sr, Rhythm::Alpha);
        let fill = occupied_band_fill(&psd);
        assert!(
            fill[IDX_FP1] > 0.8,
            "Fp1 owns the 10 Hz tone, fill={:?}",
            fill
        );
        assert!(
            fill[IDX_O1] < 0.05 && fill[IDX_O2] < 0.05,
            "O1/O2 are silent — must not inherit pair-only overlay gold, fill={:?}",
            fill
        );
        assert!(
            fill[IDX_P7] < 0.05 && fill[IDX_C3] < 0.05,
            "other silent sites stay empty of activity, fill={:?}",
            fill
        );
        assert!(psd[IDX_FP1] > psd[IDX_O1] * 10.0);

        let c3 = eight_ch_tone(n, sr, IDX_C3, 10.0);
        let c3_fill = occupied_band_fill(&channel_band_psd(&c3, sr, Rhythm::Alpha));
        assert!(c3_fill[IDX_C3] > 0.8, "C3 fill={:?}", c3_fill);
        assert!(c3_fill[IDX_O1] < 0.05 && c3_fill[IDX_O2] < 0.05);

        // 20 Hz on Fp1, 10 Hz on O1: caption-band α lights O1, not Fp1.
        let mut mixed = vec![vec![0.0; n]; 8];
        mixed[IDX_FP1] = sine(n, sr, 20.0, 10.0);
        mixed[IDX_O1] = sine(n, sr, 10.0, 10.0);
        let alpha_fill = occupied_band_fill(&channel_band_psd(&mixed, sr, Rhythm::Alpha));
        let beta_fill = occupied_band_fill(&channel_band_psd(&mixed, sr, Rhythm::Beta));
        assert!(
            alpha_fill[IDX_O1] > alpha_fill[IDX_FP1],
            "α overlay follows 10 Hz on O1, not 20 Hz on Fp1, fill={:?}",
            alpha_fill
        );
        assert!(
            beta_fill[IDX_FP1] > beta_fill[IDX_O1],
            "β overlay follows 20 Hz on Fp1, fill={:?}",
            beta_fill
        );
    }

    #[test]
    fn all_eight_occupied_sites_fill_when_band_has_power() {
        let fill = occupied_band_fill(&[1.2, 0.9, 1.0, 0.8, 1.1, 1.3, 0.7, 1.4]);
        assert!(
            fill.iter().all(|&t| t > 0.0),
            "all 8 default occupied sites need fill>0, fill={fill:?}"
        );
        let silent = occupied_band_fill(&[0.0; 8]);
        assert!(silent.iter().all(|&t| t == 0.0));
    }
}
