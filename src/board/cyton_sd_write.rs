//! Cyton on-board SD card logging (OpenBCI SDK SD commands).
//!
//! Starts a file on the card in the Cyton, independent of local Record.
//! Stop with `j`. Prefer starting before stream for a clear board reply.

/// Where transport Record writes. Hardware segmented control; Local default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RecordDestination {
    #[default]
    Local,
    Sd,
    Both,
}

impl RecordDestination {
    pub const ALL: [Self; 3] = [Self::Local, Self::Sd, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Sd => "SD",
            Self::Both => "Both",
        }
    }

    pub fn wants_local(self) -> bool {
        matches!(self, Self::Local | Self::Both)
    }

    pub fn wants_sd(self) -> bool {
        matches!(self, Self::Sd | Self::Both)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CytonSdDuration {
    Sec14,
    Min5,
    Min15,
    Min30,
    Hour1,
    Hour2,
    Hour4,
    Hour12,
    Hour24,
}

impl CytonSdDuration {
    pub const ALL: [Self; 9] = [
        Self::Sec14,
        Self::Min5,
        Self::Min15,
        Self::Min30,
        Self::Hour1,
        Self::Hour2,
        Self::Hour4,
        Self::Hour12,
        Self::Hour24,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sec14 => "14 sec",
            Self::Min5 => "5 min",
            Self::Min15 => "15 min",
            Self::Min30 => "30 min",
            Self::Hour1 => "1 hour",
            Self::Hour2 => "2 hour",
            Self::Hour4 => "4 hour",
            Self::Hour12 => "12 hour",
            Self::Hour24 => "24 hour",
        }
    }

    /// Single ASCII command the Cyton firmware expects.
    pub fn start_cmd(self) -> &'static str {
        match self {
            Self::Sec14 => "a",
            Self::Min5 => "A",
            Self::Min15 => "S",
            Self::Min30 => "F",
            Self::Hour1 => "G",
            Self::Hour2 => "H",
            Self::Hour4 => "J",
            Self::Hour12 => "K",
            Self::Hour24 => "L",
        }
    }

    pub fn stop_cmd() -> &'static str {
        "j"
    }
}

/// Fail-closed: board must confirm SD start. Empty or failure text = not recording.
pub fn sd_write_confirmed(response: &str) -> Result<(), String> {
    let trimmed = response.trim();
    let lower = trimmed.to_ascii_lowercase();
    const FAIL: &[&str] = &[
        "fail",
        "error",
        "couldn",
        "could not",
        "no card",
        "no sd",
        "init failure",
        "sd init",
        "card not",
        "not found",
        "missing",
    ];
    for needle in FAIL {
        if lower.contains(needle) {
            let msg = if trimmed.is_empty() {
                "SD write failed — check the card".to_string()
            } else {
                format!("SD write failed — {trimmed}")
            };
            return Err(msg);
        }
    }
    if trimmed.is_empty() {
        return Err(
            "Couldn't confirm SD write — check the card (best before Start)".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_commands_match_openbci_sdk() {
        assert_eq!(CytonSdDuration::Sec14.start_cmd(), "a");
        assert_eq!(CytonSdDuration::Min5.start_cmd(), "A");
        assert_eq!(CytonSdDuration::Min15.start_cmd(), "S");
        assert_eq!(CytonSdDuration::Min30.start_cmd(), "F");
        assert_eq!(CytonSdDuration::Hour1.start_cmd(), "G");
        assert_eq!(CytonSdDuration::Hour2.start_cmd(), "H");
        assert_eq!(CytonSdDuration::Hour4.start_cmd(), "J");
        assert_eq!(CytonSdDuration::Hour12.start_cmd(), "K");
        assert_eq!(CytonSdDuration::Hour24.start_cmd(), "L");
        assert_eq!(CytonSdDuration::stop_cmd(), "j");
    }

    #[test]
    fn empty_response_is_not_confirmed() {
        assert!(sd_write_confirmed("").is_err());
        assert!(sd_write_confirmed("   ").is_err());
    }

    #[test]
    fn failure_text_is_not_confirmed() {
        assert!(sd_write_confirmed("SD init failure").is_err());
        assert!(sd_write_confirmed("Could not create file").is_err());
    }

    #[test]
    fn success_text_is_confirmed() {
        assert!(sd_write_confirmed("The new filename is OBCI_01.TXT").is_ok());
    }
}
