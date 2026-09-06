//! Cyton on-board SD card logging (OpenBCI SDK SD commands).
//!
//! Starts a file on the card in the Cyton, independent of local Record.
//! Stop with `j`. Prefer starting before stream for a clear board reply.

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
}
