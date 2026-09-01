//! Ten-step spoken guided experiment.
//!
//! Holds live on an Instant clock (not packet time). Speech is fire-and-forget
//! (`say` in a background thread) so the egui thread never blocks.

use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub spoken: &'static str,
    /// `None` = no hold; enter and finish (step 10).
    pub hold: Option<Duration>,
}

/// Locked Creative Writer / Neuroscience copy. Holds are on the clock, not in the voice.
pub const STEPS: [Step; 10] = [
    Step {
        spoken: "Recording started. Sit still. Eyes open, face relaxed.",
        hold: Some(Duration::from_secs(45)),
    },
    Step {
        spoken: "Close your eyes.",
        hold: Some(Duration::from_secs(45)),
    },
    Step {
        spoken: "Open your eyes.",
        hold: Some(Duration::from_secs(20)),
    },
    Step {
        spoken: "Blink ten times.",
        hold: Some(Duration::from_secs(15)),
    },
    Step {
        spoken: "Clench your jaw. Keep your eyes open.",
        hold: Some(Duration::from_secs(10)),
    },
    Step {
        spoken: "Relax your jaw.",
        hold: Some(Duration::from_secs(10)),
    },
    Step {
        spoken: "Raise your eyebrows.",
        hold: Some(Duration::from_secs(10)),
    },
    Step {
        spoken: "Relax your face.",
        hold: Some(Duration::from_secs(10)),
    },
    Step {
        spoken: "Sit still.",
        hold: Some(Duration::from_secs(15)),
    },
    Step {
        spoken: "Recording stopped.",
        hold: None,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExperimentEvent {
    EnteredStep { index: usize, label: String },
    Finished,
    Cancelled,
}

/// Quiet chrome for the Time Series traces (current step + whisper of next).
#[derive(Clone, Debug)]
pub struct ExperimentOverlay {
    pub step_number: usize,
    pub spoken: &'static str,
    pub remaining_secs: Option<u64>,
    pub next_spoken: Option<&'static str>,
}

impl ExperimentOverlay {
    pub fn current_line(&self) -> String {
        match self.remaining_secs {
            Some(s) => format!("{}/10 {}  {s}s", self.step_number, self.spoken),
            None => format!("{}/10 {}", self.step_number, self.spoken),
        }
    }

    pub fn line(&self) -> String {
        self.current_line()
    }
}

pub fn marker_label(index: usize) -> String {
    let step = &STEPS[index];
    format!("{}/10 {}", index + 1, step.spoken)
}

/// macOS `say` in a detached process. Never call this on the egui thread with `.wait()`.
pub fn speak_detached(text: &str) {
    let text = text.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("say")
            .arg(text)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });
}

fn ceil_secs(d: Duration) -> u64 {
    let s = d.as_secs();
    if d.subsec_nanos() == 0 {
        s
    } else {
        s + 1
    }
}

pub struct ExperimentRun {
    running: bool,
    index: usize,
    step_started: Instant,
}

impl Default for ExperimentRun {
    fn default() -> Self {
        Self::new()
    }
}

impl ExperimentRun {
    pub fn new() -> Self {
        Self {
            running: false,
            index: 0,
            step_started: Instant::now(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn start(&mut self, now: Instant) -> ExperimentEvent {
        self.running = true;
        self.index = 0;
        self.step_started = now;
        ExperimentEvent::EnteredStep {
            index: 0,
            label: marker_label(0),
        }
    }

    pub fn cancel(&mut self) -> Option<ExperimentEvent> {
        if !self.running {
            return None;
        }
        self.running = false;
        Some(ExperimentEvent::Cancelled)
    }

    /// Advance on Instant elapsed. `now` is injected so tests do not sleep.
    pub fn tick(&mut self, now: Instant) -> Option<ExperimentEvent> {
        if !self.running {
            return None;
        }
        let step = STEPS[self.index];
        match step.hold {
            None => {
                self.running = false;
                Some(ExperimentEvent::Finished)
            }
            Some(hold) => {
                let elapsed = now.saturating_duration_since(self.step_started);
                if elapsed >= hold {
                    self.index += 1;
                    self.step_started = now;
                    if self.index >= STEPS.len() {
                        self.running = false;
                        return Some(ExperimentEvent::Finished);
                    }
                    Some(ExperimentEvent::EnteredStep {
                        index: self.index,
                        label: marker_label(self.index),
                    })
                } else {
                    None
                }
            }
        }
    }

    pub fn overlay(&self, now: Instant) -> Option<ExperimentOverlay> {
        if !self.running {
            return None;
        }
        let step = STEPS[self.index];
        let remaining_secs = step.hold.map(|hold| {
            ceil_secs(hold.saturating_sub(now.saturating_duration_since(self.step_started)))
        });
        Some(ExperimentOverlay {
            step_number: self.index + 1,
            spoken: step.spoken,
            remaining_secs,
            next_spoken: STEPS.get(self.index + 1).map(|s| s.spoken),
        })
    }

    pub fn overlay_lines(&self, now: Instant) -> Option<(String, Option<String>)> {
        let ov = self.overlay(now)?;
        Some((ov.current_line(), ov.next_spoken.map(|s| s.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_in_locked_order() {
        let holds: Vec<Option<u64>> = STEPS.iter().map(|s| s.hold.map(|d| d.as_secs())).collect();
        assert_eq!(
            holds,
            vec![
                Some(45),
                Some(45),
                Some(20),
                Some(15),
                Some(10),
                Some(10),
                Some(10),
                Some(10),
                Some(15),
                None,
            ]
        );
        assert_eq!(STEPS[9].spoken, "Recording stopped.");
        assert!(STEPS[9].hold.is_none());
    }

    #[test]
    fn marker_labels_equal_spoken_lines() {
        let expected = [
            "Recording started. Sit still. Eyes open, face relaxed.",
            "Close your eyes.",
            "Open your eyes.",
            "Blink ten times.",
            "Clench your jaw. Keep your eyes open.",
            "Relax your jaw.",
            "Raise your eyebrows.",
            "Relax your face.",
            "Sit still.",
            "Recording stopped.",
        ];
        for (i, line) in expected.iter().enumerate() {
            assert_eq!(STEPS[i].spoken, *line);
            let label = marker_label(i);
            assert_eq!(label, format!("{}/10 {line}", i + 1));
            assert!(label.contains(line));
        }
    }

    #[test]
    fn tick_advances_after_each_hold_then_step_ten_finishes() {
        let mut run = ExperimentRun::new();
        let t0 = Instant::now();
        match run.start(t0) {
            ExperimentEvent::EnteredStep { index, label } => {
                assert_eq!(index, 0);
                assert_eq!(
                    label,
                    "1/10 Recording started. Sit still. Eyes open, face relaxed."
                );
            }
            other => panic!("expected enter step 1, got {other:?}"),
        }
        assert!(run.is_running());
        assert_eq!(run.tick(t0), None);

        let holds = [45u64, 45, 20, 15, 10, 10, 10, 10, 15];
        let mut t = t0;
        for (i, &h) in holds.iter().enumerate() {
            assert_eq!(
                run.tick(t + Duration::from_millis(h * 1000 - 1)),
                None,
                "must not advance before hold {h}s on step {}",
                i + 1
            );
            t += Duration::from_secs(h);
            match run.tick(t) {
                Some(ExperimentEvent::EnteredStep { index, label }) => {
                    assert_eq!(index, i + 1);
                    assert_eq!(label, marker_label(i + 1));
                    assert!(label.contains(STEPS[i + 1].spoken));
                }
                other => panic!("expected enter step {}, got {other:?}", i + 2),
            }
        }

        assert_eq!(run.index(), 9);
        assert!(STEPS[run.index()].hold.is_none());
        match run.tick(t) {
            Some(ExperimentEvent::Finished) => {}
            other => panic!("step 10 has no hold and must finish, got {other:?}"),
        }
        assert!(!run.is_running());
        assert!(run.tick(t + Duration::from_secs(1)).is_none());
    }

    #[test]
    fn overlay_clock_is_instant_elapsed() {
        let mut run = ExperimentRun::new();
        let t0 = Instant::now();
        let _ = run.start(t0);
        let ov = run.overlay(t0).expect("running");
        assert_eq!(ov.step_number, 1);
        assert_eq!(ov.remaining_secs, Some(45));
        assert_eq!(ov.next_spoken, Some("Close your eyes."));
        assert_eq!(
            ov.current_line(),
            "1/10 Recording started. Sit still. Eyes open, face relaxed.  45s"
        );
        let ov = run.overlay(t0 + Duration::from_secs(10)).unwrap();
        assert_eq!(ov.remaining_secs, Some(35));
        assert!(
            ov.current_line().ends_with("  35s"),
            "{}",
            ov.current_line()
        );
        let ev = run.tick(t0 + Duration::from_secs(45)).unwrap();
        assert!(matches!(ev, ExperimentEvent::EnteredStep { index: 1, .. }));
    }

    #[test]
    fn cancel_clears_running_state() {
        let mut run = ExperimentRun::new();
        let t0 = Instant::now();
        let _ = run.start(t0);
        assert_eq!(run.cancel(), Some(ExperimentEvent::Cancelled));
        assert!(!run.is_running());
        assert!(run.cancel().is_none());
        assert!(run.overlay(t0).is_none());
    }
}
