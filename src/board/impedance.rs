//! ADS1299 / Ganglion impedance helpers (Java `BoardCyton` + `DataProcessing`).
//!
//! Cyton has no BrainFlow `resistance_channels`. kΩ is the 1 s population std of
//! raw EXG (µV) with the 6 nA lead-off current on, same formula as the Java GUI.
//! Only one channel is injected at a time: impedance mode disconnects SRB2, and
//! doing that on every channel at once would drop the reference.

use std::f64::consts::SQRT_2;

/// Cyton 32-bit series resistor (Java `BoardCytonConstants.series_resistor_ohms`).
pub const SERIES_RESISTOR_OHMS: f64 = 2200.0;
/// ADS1299 lead-off drive (Java `BoardCytonConstants.leadOffDrive_amps`).
pub const LEAD_OFF_AMPS: f64 = 6.0e-9;

/// Cyton / Daisy channel letters for `x…X` / `z…Z` (Java `channelSelectForSettings`).
const ADS_CHANNEL_LETTERS: [char; 16] = [
    '1', '2', '3', '4', '5', '6', '7', '8', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I',
];

pub fn ads_channel_letter(channel: usize) -> Option<char> {
    ADS_CHANNEL_LETTERS.get(channel).copied()
}

/// Gain x1, bias on, SRB2/SRB1 off, N-pin lead-off. Java `setCheckingImpedanceCyton(…, true, N)`.
pub fn cyton_impedance_on_cmd(channel: usize) -> Option<String> {
    let c = ads_channel_letter(channel)?;
    Some(format!("x{c}000100Xz{c}01Z"))
}

/// Restore default ADS (gain x24, SRB2 on) and clear lead-off.
#[allow(dead_code)]
pub fn cyton_impedance_off_cmd(channel: usize) -> Option<String> {
    let c = ads_channel_letter(channel)?;
    Some(format!("x{c}061100Xz{c}00Z"))
}

/// Split concatenated Cyton ADS / lead-off commands so each `config_board` call
/// gets one terminator. `x1000100Xz101Z` → `x1000100X` then `z101Z`.
/// Commands with no trailing X/Z (`z`, `/2`) stay a single piece.
pub fn split_cyton_config_cmds(cmd: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, ch) in cmd.char_indices() {
        if ch == 'X' || ch == 'Z' {
            let end = i + ch.len_utf8();
            if end > start {
                out.push(&cmd[start..end]);
            }
            start = end;
        }
    }
    if start < cmd.len() {
        out.push(&cmd[start..]);
    }
    out
}

/// Population std (Java `std()` uses `/ n`, not `n-1`).
pub fn population_std(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    Some(var.sqrt())
}

/// Java `data_elec_imp_ohm / 1000` from 1 s std of the EXG column in µV.
pub fn kohm_from_lead_off_std_uv(std_uv: f64) -> f64 {
    if !std_uv.is_finite() {
        return 0.0;
    }
    let mut ohms = (SQRT_2 * std_uv * 1.0e-6) / LEAD_OFF_AMPS - SERIES_RESISTOR_OHMS;
    if ohms < 0.0 {
        ohms = 0.0;
    }
    ohms / 1000.0
}

/// Last `window` samples of BrainFlow column `col`, or None if too short.
pub fn column_window(rows: &[Vec<f64>], col: usize, window: usize) -> Option<Vec<f64>> {
    if window == 0 || rows.len() < window / 2 {
        return None;
    }
    let start = rows.len().saturating_sub(window);
    Some(
        rows[start..]
            .iter()
            .map(|row| row.get(col).copied().unwrap_or(0.0))
            .collect(),
    )
}

/// Ganglion resistance columns are kΩ; Java divides by 2 (driven ground ≈ electrode).
pub fn ganglion_kohm(resistance: f64) -> Option<f64> {
    if resistance.is_finite() && resistance > 0.0 {
        Some(resistance / 2.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ads_letters_match_java() {
        assert_eq!(ads_channel_letter(0), Some('1'));
        assert_eq!(ads_channel_letter(7), Some('8'));
        assert_eq!(ads_channel_letter(8), Some('Q'));
        assert_eq!(ads_channel_letter(15), Some('I'));
        assert_eq!(ads_channel_letter(16), None);
    }

    #[test]
    fn cyton_on_off_commands() {
        assert_eq!(cyton_impedance_on_cmd(0).as_deref(), Some("x1000100Xz101Z"));
        assert_eq!(
            cyton_impedance_off_cmd(0).as_deref(),
            Some("x1061100Xz100Z")
        );
        assert_eq!(cyton_impedance_on_cmd(3).as_deref(), Some("x4000100Xz401Z"));
        assert_eq!(
            cyton_impedance_off_cmd(8).as_deref(),
            Some("xQ061100XzQ00Z")
        );
    }

    #[test]
    fn split_cyton_impedance_cmds_on_trailing_xz() {
        assert_eq!(
            split_cyton_config_cmds("x1000100Xz101Z"),
            vec!["x1000100X", "z101Z"]
        );
        assert_eq!(
            split_cyton_config_cmds("x1061100Xz100Z"),
            vec!["x1061100X", "z100Z"]
        );
        assert_eq!(
            split_cyton_config_cmds(cyton_impedance_on_cmd(0).as_deref().unwrap()),
            vec!["x1000100X", "z101Z"]
        );
        assert_eq!(
            split_cyton_config_cmds(cyton_impedance_off_cmd(0).as_deref().unwrap()),
            vec!["x1061100X", "z100Z"]
        );
        assert_eq!(split_cyton_config_cmds("x1060110X"), vec!["x1060110X"]);
        assert_eq!(split_cyton_config_cmds("z101Z"), vec!["z101Z"]);
        assert_eq!(split_cyton_config_cmds("z"), vec!["z"]);
        assert_eq!(split_cyton_config_cmds(""), Vec::<&str>::new());
    }

    #[test]
    fn kohm_formula_matches_java() {
        // std 0 → clamp at 0 kΩ (below the 2.2 kΩ series resistor).
        assert_eq!(kohm_from_lead_off_std_uv(0.0), 0.0);
        // (√2 * 50e-6) / 6e-9 - 2200 = 9578.64 Ω → 9.578 kΩ
        let k = kohm_from_lead_off_std_uv(50.0);
        assert!((k - 9.578).abs() < 0.01, "got {k}");
    }

    #[test]
    fn population_std_constant_is_zero() {
        let xs = [3.0, 3.0, 3.0, 3.0];
        assert!(population_std(&xs).unwrap().abs() < 1e-12);
        assert!(population_std(&[1.0]).is_none());
    }

    #[test]
    fn ganglion_halves_positive_resistance() {
        assert_eq!(ganglion_kohm(20.0), Some(10.0));
        assert_eq!(ganglion_kohm(0.0), None);
        assert_eq!(ganglion_kohm(-1.0), None);
    }

    #[test]
    fn column_window_takes_tail() {
        let rows = vec![
            vec![0.0, 1.0],
            vec![0.0, 2.0],
            vec![0.0, 3.0],
            vec![0.0, 4.0],
        ];
        assert_eq!(column_window(&rows, 1, 2), Some(vec![3.0, 4.0]));
        assert!(column_window(&rows, 1, 20).is_none());
    }
}
