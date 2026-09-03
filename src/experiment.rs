//! Guided recording (ten-step) and Eyes closed (second protocol).
//!
//! Holds live on an Instant clock (not packet time). Speech is fire-and-forget
//! (`say` in a background thread) so the egui thread never blocks. Bells are
//! `afplay` system sounds, never a spoken word and never a trace label.

use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolKind {
    Guided,
    EyesClosed,
}

impl ProtocolKind {
    pub fn label(self) -> &'static str {
        match self {
            ProtocolKind::Guided => "Guided recording",
            ProtocolKind::EyesClosed => "Eyes closed",
        }
    }

    pub fn steps(self) -> &'static [Step] {
        match self {
            ProtocolKind::Guided => &STEPS,
            ProtocolKind::EyesClosed => &EYES_CLOSED,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Speak,
    SpeakThenBell,
    BellThenSpeak,
    Silence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub spoken: &'static str,
    /// `None` = no hold; enter and finish (guided step 10).
    pub hold: Option<Duration>,
    pub file_mark: Option<&'static str>,
    pub cue: Cue,
}

const fn guided(spoken: &'static str, hold: Option<Duration>) -> Step {
    Step {
        spoken,
        hold,
        file_mark: None,
        cue: Cue::Speak,
    }
}

/// Locked Creative Writer / Neuroscience copy. Holds are on the clock, not in the voice.
pub const STEPS: [Step; 10] = [
    guided(
        "Recording started. Sit still. Eyes open, face relaxed.",
        Some(Duration::from_secs(45)),
    ),
    guided("Close your eyes.", Some(Duration::from_secs(45))),
    guided("Open your eyes.", Some(Duration::from_secs(20))),
    guided("Blink ten times.", Some(Duration::from_secs(15))),
    guided(
        "Clench your jaw. Keep your eyes open.",
        Some(Duration::from_secs(10)),
    ),
    guided("Relax your jaw.", Some(Duration::from_secs(10))),
    guided("Raise your eyebrows.", Some(Duration::from_secs(10))),
    guided("Relax your face.", Some(Duration::from_secs(10))),
    guided("Sit still.", Some(Duration::from_secs(15))),
    guided("Recording stopped.", None),
];

/// Eyes closed: spoken as-is. File marks only sit still · close eyes · open eyes · sit still.
pub const EYES_CLOSED: [Step; 5] = [
    Step {
        spoken: "Sit still. Eyes open. Face and jaw relaxed. Take a minute to prepare for a ten-minute meditation.",
        hold: Some(Duration::from_secs(60)),
        file_mark: Some("sit still"),
        cue: Cue::Speak,
    },
    Step {
        spoken: "Close your eyes.",
        hold: Some(Duration::from_secs(4)),
        file_mark: Some("close eyes"),
        cue: Cue::SpeakThenBell,
    },
    Step {
        spoken: "",
        hold: Some(Duration::from_secs(600)),
        file_mark: None,
        cue: Cue::Silence,
    },
    Step {
        spoken: "Open your eyes.",
        hold: Some(Duration::from_secs(5)),
        file_mark: Some("open eyes"),
        cue: Cue::BellThenSpeak,
    },
    Step {
        spoken: "Sit still.",
        hold: Some(Duration::from_secs(20)),
        file_mark: Some("sit still"),
        cue: Cue::Speak,
    },
];

pub const BELL_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExperimentEvent {
    EnteredStep { index: usize, label: String },
    Finished,
    Cancelled,
}

/// Quiet chrome for the Time Series traces: only the current spoken beat.
#[derive(Clone, Debug)]
pub struct ExperimentOverlay {
    pub step_number: usize,
    pub spoken: &'static str,
    pub remaining_secs: Option<u64>,
    pub next_spoken: Option<&'static str>,
}

pub fn format_remaining(secs: u64) -> String {
    if secs >= 60 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl ExperimentOverlay {
    pub fn current_line(&self) -> String {
        let beat = self.spoken.trim();
        match (beat.is_empty(), self.remaining_secs) {
            (true, Some(s)) => format_remaining(s),
            (true, None) => String::new(),
            (false, Some(s)) => format!("{beat}  {}", format_remaining(s)),
            (false, None) => beat.to_string(),
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

pub fn file_mark(protocol: ProtocolKind, index: usize) -> String {
    let steps = protocol.steps();
    match protocol {
        ProtocolKind::Guided => {
            if index < steps.len() {
                marker_label(index)
            } else {
                String::new()
            }
        }
        ProtocolKind::EyesClosed => steps
            .get(index)
            .and_then(|s| s.file_mark)
            .unwrap_or("")
            .to_string(),
    }
}

/// macOS `say` in a detached process. Never call this on the egui thread with `.wait()`.
pub fn speak_detached(text: &str) {
    if text.trim().is_empty() {
        return;
    }
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

pub fn play_bell_blocking() {
    let _ = Command::new("afplay")
        .arg(BELL_SOUND)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

pub fn play_step_cue(step: Step) {
    match step.cue {
        Cue::Silence => {}
        Cue::Speak => speak_detached(step.spoken),
        Cue::SpeakThenBell => {
            let text = step.spoken.to_string();
            std::thread::spawn(move || {
                if !text.trim().is_empty() {
                    let _ = Command::new("say")
                        .arg(&text)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
                play_bell_blocking();
            });
        }
        Cue::BellThenSpeak => {
            let text = step.spoken.to_string();
            std::thread::spawn(move || {
                play_bell_blocking();
                if !text.trim().is_empty() {
                    let _ = Command::new("say")
                        .arg(&text)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            });
        }
    }
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
    protocol: ProtocolKind,
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
            protocol: ProtocolKind::Guided,
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

    pub fn protocol(&self) -> ProtocolKind {
        self.protocol
    }

    pub fn current_step(&self) -> Option<Step> {
        self.protocol.steps().get(self.index).copied()
    }

    pub fn start(&mut self, now: Instant) -> ExperimentEvent {
        self.start_protocol(ProtocolKind::Guided, now)
    }

    pub fn start_protocol(&mut self, protocol: ProtocolKind, now: Instant) -> ExperimentEvent {
        self.running = true;
        self.protocol = protocol;
        self.index = 0;
        self.step_started = now;
        ExperimentEvent::EnteredStep {
            index: 0,
            label: file_mark(protocol, 0),
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
        let steps = self.protocol.steps();
        let step = steps[self.index];
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
                    if self.index >= steps.len() {
                        self.running = false;
                        return Some(ExperimentEvent::Finished);
                    }
                    Some(ExperimentEvent::EnteredStep {
                        index: self.index,
                        label: file_mark(self.protocol, self.index),
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
        let steps = self.protocol.steps();
        let step = steps[self.index];
        let remaining_secs = step.hold.map(|hold| {
            ceil_secs(hold.saturating_sub(now.saturating_duration_since(self.step_started)))
        });
        Some(ExperimentOverlay {
            step_number: self.index + 1,
            spoken: step.spoken,
            remaining_secs,
            next_spoken: None,
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
        assert_eq!(ov.next_spoken, None);
        assert_eq!(
            ov.current_line(),
            "Recording started. Sit still. Eyes open, face relaxed.  45s"
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

    #[test]
    fn eyes_closed_spoken_holds_and_file_marks() {
        assert_eq!(
            EYES_CLOSED[0].spoken,
            "Sit still. Eyes open. Face and jaw relaxed. Take a minute to prepare for a ten-minute meditation."
        );
        assert_eq!(EYES_CLOSED[0].hold, Some(Duration::from_secs(60)));
        assert_eq!(EYES_CLOSED[0].file_mark, Some("sit still"));
        assert_eq!(EYES_CLOSED[1].spoken, "Close your eyes.");
        assert_eq!(EYES_CLOSED[1].cue, Cue::SpeakThenBell);
        assert_eq!(EYES_CLOSED[1].file_mark, Some("close eyes"));
        assert_eq!(EYES_CLOSED[2].spoken, "");
        assert_eq!(EYES_CLOSED[2].hold, Some(Duration::from_secs(600)));
        assert_eq!(EYES_CLOSED[2].cue, Cue::Silence);
        assert_eq!(EYES_CLOSED[2].file_mark, None);
        assert_eq!(EYES_CLOSED[3].spoken, "Open your eyes.");
        assert_eq!(EYES_CLOSED[3].cue, Cue::BellThenSpeak);
        assert_eq!(EYES_CLOSED[3].file_mark, Some("open eyes"));
        assert_eq!(EYES_CLOSED[4].spoken, "Sit still.");
        assert_eq!(EYES_CLOSED[4].hold, Some(Duration::from_secs(20)));
        assert_eq!(EYES_CLOSED[4].file_mark, Some("sit still"));
        let marks: Vec<_> = (0..5)
            .map(|i| file_mark(ProtocolKind::EyesClosed, i))
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(
            marks,
            vec!["sit still", "close eyes", "open eyes", "sit still"]
        );
        assert_eq!(BELL_SOUND, "/System/Library/Sounds/Glass.aiff");
    }

    #[test]
    fn eyes_closed_silence_overlay_is_remaining_only() {
        let mut run = ExperimentRun::new();
        let t0 = Instant::now();
        match run.start_protocol(ProtocolKind::EyesClosed, t0) {
            ExperimentEvent::EnteredStep { label, .. } => assert_eq!(label, "sit still"),
            other => panic!("{other:?}"),
        }
        let ov = run.overlay(t0).unwrap();
        assert!(ov.current_line().contains("Sit still. Eyes open"));
        assert_eq!(ov.next_spoken, None);
        let _ = run.tick(t0 + Duration::from_secs(60));
        let _ = run.tick(t0 + Duration::from_secs(64));
        assert_eq!(run.index(), 2);
        let ov = run.overlay(t0 + Duration::from_secs(64)).unwrap();
        assert!(ov.spoken.is_empty());
        assert_eq!(ov.remaining_secs, Some(600));
        assert_eq!(ov.current_line(), "10:00");
        assert!(!ov.current_line().to_lowercase().contains("bell"));
    }

    #[test]
    fn eyes_closed_silence_remaining_is_wall_clock_when_samples_missing() {
        let mut run = ExperimentRun::new();
        let t0 = Instant::now();
        let _ = run.start_protocol(ProtocolKind::EyesClosed, t0);
        assert!(run.tick(t0 + Duration::from_secs(60)).is_some());
        assert!(run.tick(t0 + Duration::from_secs(64)).is_some());
        assert_eq!(run.index(), 2);
        // No samples for a minute of wall time: remaining is 9:00, not a skip.
        let ov = run
            .overlay(t0 + Duration::from_secs(64 + 60))
            .expect("silence");
        assert_eq!(ov.remaining_secs, Some(540));
        assert_eq!(ov.current_line(), "9:00");
        assert!(run.tick(t0 + Duration::from_secs(64 + 60)).is_none());
        assert!(run.tick(t0 + Duration::from_secs(64 + 599)).is_none());
        match run.tick(t0 + Duration::from_secs(64 + 600)) {
            Some(ExperimentEvent::EnteredStep { label, .. }) => {
                assert_eq!(label, "open eyes");
            }
            other => panic!("silence is 600s wall, got {other:?}"),
        }
    }
}
