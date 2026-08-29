//! Stable stream-rate / packet-loss strings for the top bar.
//!
//! The previous UI only drew "Loss: …" when the instantaneous % was > 0.5, so the
//! label popped in and out and shoved the rest of the bar. These helpers keep a
//! fixed-width readout and a lightly smoothed value.

use eframe::egui::Color32;

const HZ_WIDTH: usize = 6;
const LOSS_WIDTH: usize = 5;

pub fn format_hz(hz: f64) -> String {
    format!("{:>width$.1} Hz", hz.max(0.0), width = HZ_WIDTH)
}

pub fn format_loss(percent: f64) -> String {
    format!(
        "Loss {:>width$.1}%",
        percent.clamp(0.0, 100.0),
        width = LOSS_WIDTH
    )
}

pub fn loss_color(percent: f64) -> Color32 {
    if percent > 5.0 {
        Color32::from_rgb(255, 90, 90)
    } else if percent > 1.0 {
        Color32::from_rgb(255, 200, 80)
    } else {
        Color32::from_rgb(180, 220, 190)
    }
}

/// Exponential moving average. `alpha` is the weight of the new sample (0..1).
pub fn smooth(previous: f64, sample: f64, alpha: f64) -> f64 {
    let a = alpha.clamp(0.0, 1.0);
    previous * (1.0 - a) + sample * a
}

pub fn loss_percent(received: u64, lost: u64) -> f64 {
    let expected = received.saturating_add(lost);
    if expected == 0 {
        0.0
    } else {
        lost as f64 / expected as f64 * 100.0
    }
}

/// Java `PacketLossTracker`: count gaps in the board sample-index sequence.
/// Wall-clock vs sample-rate is not packet loss (a slow UI frame looks like a drop).
pub struct SampleIndexTracker {
    last: Option<i32>,
    sequence: Vec<i32>,
    window_received: u64,
    window_lost: u64,
}

impl SampleIndexTracker {
    pub fn wrapping_0_255() -> Self {
        Self::from_sequence((0..=255).collect())
    }

    /// Cyton + Daisy serial: even indices 0,2,…,254.
    pub fn daisy_even_0_254() -> Self {
        Self::from_sequence((0..=254).step_by(2).map(|i| i as i32).collect())
    }

    fn from_sequence(sequence: Vec<i32>) -> Self {
        Self {
            last: None,
            sequence,
            window_received: 0,
            window_lost: 0,
        }
    }

    pub fn observe(&mut self, index: i32) -> u64 {
        self.window_received += 1;
        let lost = match self.last {
            None => 0,
            Some(prev) => index_gap(prev, index, &self.sequence),
        };
        self.window_lost += lost;
        self.last = Some(index);
        lost
    }

    pub fn take_window(&mut self) -> (u64, u64) {
        let out = (self.window_received, self.window_lost);
        self.window_received = 0;
        self.window_lost = 0;
        out
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.window_received = 0;
        self.window_lost = 0;
    }
}

fn index_gap(from: i32, to: i32, seq: &[i32]) -> u64 {
    if seq.is_empty() {
        return 0;
    }
    let Some(start) = seq.iter().position(|&x| x == from) else {
        return 0;
    };
    let mut i = start;
    let mut lost = 0u64;
    loop {
        i = (i + 1) % seq.len();
        if seq[i] == to {
            break;
        }
        lost += 1;
        if lost as usize >= seq.len() {
            break;
        }
    }
    lost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hz_string_width_is_stable() {
        let a = format_hz(0.0);
        let b = format_hz(192.9);
        let c = format_hz(250.0);
        assert_eq!(a.chars().count(), b.chars().count());
        assert_eq!(b.chars().count(), c.chars().count());
        assert!(a.ends_with(" Hz"));
    }

    #[test]
    fn loss_string_width_is_stable() {
        let hidden_style_zero = format_loss(0.0);
        let flicker_candidate = format_loss(0.6);
        let high = format_loss(12.5);
        assert_eq!(
            hidden_style_zero.chars().count(),
            flicker_candidate.chars().count()
        );
        assert_eq!(flicker_candidate.chars().count(), high.chars().count());
        assert!(hidden_style_zero.starts_with("Loss "));
    }

    #[test]
    fn smooth_does_not_jump_to_sample_in_one_step() {
        let next = smooth(0.0, 20.0, 0.3);
        assert!(next > 0.0);
        assert!(next < 20.0);
    }

    #[test]
    fn consecutive_indices_are_zero_loss() {
        let mut t = SampleIndexTracker::wrapping_0_255();
        for i in 0..20 {
            t.observe(i);
        }
        let (recv, lost) = t.take_window();
        assert_eq!(recv, 20);
        assert_eq!(lost, 0);
    }

    #[test]
    fn wrap_from_255_to_0_is_not_loss() {
        let mut t = SampleIndexTracker::wrapping_0_255();
        t.observe(254);
        t.observe(255);
        t.observe(0);
        t.observe(1);
        let (recv, lost) = t.take_window();
        assert_eq!(recv, 4);
        assert_eq!(lost, 0);
    }

    #[test]
    fn gap_of_three_counts_three_lost() {
        let mut t = SampleIndexTracker::wrapping_0_255();
        t.observe(10);
        t.observe(14); // missed 11,12,13
        let (recv, lost) = t.take_window();
        assert_eq!(recv, 2);
        assert_eq!(lost, 3);
        let pct = loss_percent(recv, lost);
        assert!((pct - 60.0).abs() < 1e-9, "pct={}", pct);
    }

    #[test]
    fn wall_clock_hitch_is_not_index_loss() {
        // 250 samples in order, even if they arrived after a 200ms UI stall.
        let mut t = SampleIndexTracker::wrapping_0_255();
        for i in 0..250 {
            t.observe(i);
        }
        let (recv, lost) = t.take_window();
        assert_eq!(lost, 0);
        assert_eq!(loss_percent(recv, lost), 0.0);
    }

    #[test]
    fn daisy_even_sequence_has_no_false_loss() {
        let mut t = SampleIndexTracker::daisy_even_0_254();
        t.observe(0);
        t.observe(2);
        t.observe(4);
        let (_, lost) = t.take_window();
        assert_eq!(lost, 0);
    }
}
