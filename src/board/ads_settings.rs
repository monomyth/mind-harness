//! Cyton ADS1299 channel commands (Java `ADS1299Settings.commit`).
//!
//! `x{letter}{power}{gain}{input}{bias}{srb2}{srb1}X`

use super::impedance::ads_channel_letter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AdsPower {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AdsGain {
    X1,
    X2,
    X4,
    X6,
    X8,
    X12,
    X24,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AdsInput {
    Normal,
    Shorted,
    BiasMeas,
    Mvdd,
    Temp,
    Test,
    BiasDrp,
    BiasDrn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AdsYesNo {
    No,
    Yes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdsChannel {
    pub power: AdsPower,
    pub gain: AdsGain,
    pub input: AdsInput,
    pub bias: AdsYesNo,
    pub srb2: AdsYesNo,
    pub srb1: AdsYesNo,
}

impl Default for AdsChannel {
    fn default() -> Self {
        Self {
            power: AdsPower::On,
            gain: AdsGain::X24,
            input: AdsInput::Normal,
            bias: AdsYesNo::Yes,
            srb2: AdsYesNo::Yes,
            srb1: AdsYesNo::No,
        }
    }
}

impl AdsChannel {
    fn ordinals(self) -> (u8, u8, u8, u8, u8, u8) {
        (
            self.power as u8,
            self.gain as u8,
            self.input as u8,
            self.bias as u8,
            self.srb2 as u8,
            self.srb1 as u8,
        )
    }
}

/// Java `String.format("x%c%d%d%d%d%d%dX", …)`.
pub fn ads_commit_cmd(channel: usize, s: AdsChannel) -> Option<String> {
    let c = ads_channel_letter(channel)?;
    let (p, g, i, b, s2, s1) = s.ordinals();
    Some(format!("x{c}{p}{g}{i}{b}{s2}{s1}X"))
}

/// Restore this channel's ADS settings and clear N-pin lead-off (`z…00Z`).
pub fn ads_impedance_restore_cmd(channel: usize, s: AdsChannel) -> Option<String> {
    let commit = ads_commit_cmd(channel, s)?;
    let c = ads_channel_letter(channel)?;
    Some(format!("{commit}z{c}00Z"))
}

pub fn default_bank(n: usize) -> Vec<AdsChannel> {
    vec![AdsChannel::default(); n.clamp(1, 16)]
}

/// Zero EXG columns whose ADS power digit is Off (Time Series drop / Synthetic emulate).
pub fn zero_unpowered_exg(row: &mut [f64], exg_channels: &[usize], bank: &[AdsChannel]) {
    for (i, &col) in exg_channels.iter().enumerate() {
        if bank
            .get(i)
            .map(|s| s.power == AdsPower::Off)
            .unwrap_or(false)
        {
            if let Some(v) = row.get_mut(col) {
                *v = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_default_channel_command_is_060110() {
        let cmd = ads_commit_cmd(0, AdsChannel::default()).unwrap();
        assert_eq!(cmd, "x1060110X");
    }

    #[test]
    fn powering_off_channel_8_sets_power_digit() {
        let s = AdsChannel {
            power: AdsPower::Off,
            ..AdsChannel::default()
        };
        assert_eq!(ads_commit_cmd(7, s).unwrap(), "x8160110X");
    }

    #[test]
    fn daisy_channel_9_uses_q() {
        assert_eq!(
            ads_commit_cmd(8, AdsChannel::default()).unwrap(),
            "xQ060110X"
        );
    }

    #[test]
    fn impedance_restore_uses_live_ads_not_only_default() {
        let s = AdsChannel {
            power: AdsPower::Off,
            ..AdsChannel::default()
        };
        assert_eq!(ads_impedance_restore_cmd(7, s).unwrap(), "x8160110Xz800Z");
    }

    #[test]
    fn power_off_zeros_that_exg_column_only() {
        let mut row = vec![9.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let exg = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut bank = default_bank(8);
        bank[7].power = AdsPower::Off;
        zero_unpowered_exg(&mut row, &exg, &bank);
        assert_eq!(row[8], 0.0);
        assert_eq!(row[1], 1.0);
        assert_eq!(row[7], 7.0);
    }
}
