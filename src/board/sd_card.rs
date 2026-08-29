//! Cyton SD card hex dump → Playback rows (Java `DataSourceSDCard`).
//!
//! One well-tested layout: comma-separated hex (`sample, ch0..ch7[, ch8..ch15], ax, ay, az`).

use crate::board::BoardError;

/// Java `BoardCytonConstants.scale_fac_uVolts_per_count`.
pub const SCALE_UV_PER_COUNT: f64 = 4.5 / (((1 << 23) as f64) - 1.0) / 24.0 * 1_000_000.0;
/// Java `BoardCytonConstants.accelScale`.
pub const ACCEL_SCALE: f64 = 0.002 / 16.0;

/// Java `parseInt24Hex`: 24-bit two's complement from 6 hex digits.
pub fn parse_int24_hex(hex: &str) -> Result<i32, BoardError> {
    let h = hex.trim();
    if h.is_empty() {
        return Err(BoardError::Io("empty 24-bit hex field".into()));
    }
    let first = h.chars().next().unwrap();
    let padded = if first > '7' {
        format!("FF{h}")
    } else {
        format!("00{h}")
    };
    u32::from_str_radix(&padded, 16)
        .map(|v| v as i32)
        .map_err(|e| BoardError::Io(format!("int24 hex {h}: {e}")))
}

/// Java `parseInt16Hex`.
pub fn parse_int16_hex(hex: &str) -> Result<i32, BoardError> {
    let h = hex.trim();
    if h.is_empty() {
        return Err(BoardError::Io("empty 16-bit hex field".into()));
    }
    let first = h.chars().next().unwrap();
    let padded = if first > '7' {
        format!("FFFF{h}")
    } else {
        format!("0000{h}")
    };
    u32::from_str_radix(&padded, 16)
        .map(|v| v as i32)
        .map_err(|e| BoardError::Io(format!("int16 hex {h}: {e}")))
}

/// One parsed SD row as PlaybackBoard wants it: EXG µV then accel then timestamp.
pub fn parse_sd_row(line: &str) -> Result<(Vec<f64>, usize), BoardError> {
    let parts: Vec<&str> = line
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 8 {
        return Err(BoardError::Io(format!(
            "SD row too short ({} fields)",
            parts.len()
        )));
    }
    let n_exg = if parts.len() < 15 { 8 } else { 16 };
    if parts.len() < n_exg + 1 {
        return Err(BoardError::Io("SD row missing EXG columns".into()));
    }
    let mut row = Vec::with_capacity(n_exg + 4);
    // Java stores sample index at [0] then EXG at [1..]; Playback uses EXG-first rows.
    let _idx = parse_int24_hex(parts[0]).or_else(|_| {
        i32::from_str_radix(parts[0], 16).map_err(|e| BoardError::Io(e.to_string()))
    })?;
    for p in parts.iter().take(n_exg + 1).skip(1) {
        let counts = parse_int24_hex(p)?;
        row.push(counts as f64 * SCALE_UV_PER_COUNT);
    }
    let mut ax = 0.0;
    let mut ay = 0.0;
    let mut az = 0.0;
    if parts.len() >= n_exg + 4 {
        ax = parse_int16_hex(parts[n_exg + 1])? as f64 * ACCEL_SCALE;
        ay = parse_int16_hex(parts[n_exg + 2])? as f64 * ACCEL_SCALE;
        az = parse_int16_hex(parts[n_exg + 3])? as f64 * ACCEL_SCALE;
    }
    row.push(ax);
    row.push(ay);
    row.push(az);
    Ok((row, n_exg))
}

pub fn parse_sd_file(path: &std::path::Path) -> Result<(Vec<Vec<f64>>, usize, i32), BoardError> {
    let text = std::fs::read_to_string(path).map_err(|e| BoardError::Io(e.to_string()))?;
    parse_sd_text(&text)
}

pub fn parse_sd_text(text: &str) -> Result<(Vec<Vec<f64>>, usize, i32), BoardError> {
    let mut samples = Vec::new();
    let mut n_exg = 8usize;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('%') || trimmed.starts_with('#') {
            continue;
        }
        match parse_sd_row(trimmed) {
            Ok((row, n)) => {
                n_exg = n;
                samples.push(row);
            }
            Err(e) => {
                if samples.is_empty() && i < 8 {
                    continue;
                }
                return Err(BoardError::Io(format!("SD line {}: {e}", i + 1)));
            }
        }
    }
    if samples.is_empty() {
        return Err(BoardError::Io(
            "No Cyton SD hex rows found (need comma-separated 24-bit hex)".into(),
        ));
    }
    Ok((samples, n_exg, 250))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int24_negative_sign_extends() {
        // 0x800000 → -8388608
        assert_eq!(parse_int24_hex("800000").unwrap(), -8388608);
        assert_eq!(parse_int24_hex("000001").unwrap(), 1);
        assert_eq!(parse_int24_hex("FFFFFF").unwrap(), -1);
    }

    #[test]
    fn eight_channel_row_converts_counts_to_uv() {
        // index + 8 channels of 000001 + dummy accel
        let line = "00,000001,000001,000001,000001,000001,000001,000001,000001,0000,0000,0000";
        let (row, n) = parse_sd_row(line).unwrap();
        assert_eq!(n, 8);
        assert_eq!(row.len(), 11);
        let expected = SCALE_UV_PER_COUNT;
        assert!((row[0] - expected).abs() < 1e-9);
        assert_eq!(row[8], 0.0);
    }

    #[test]
    fn short_line_is_a_readable_error() {
        let err = parse_sd_text("not,hex").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("SD") || msg.contains("hex") || msg.contains("short"));
    }

    #[test]
    fn parse_file_skips_comments() {
        let text = "% header\n00,000001,000002,000003,000004,000005,000006,000007,000008\n";
        let (rows, n, sr) = parse_sd_text(text).unwrap();
        assert_eq!(n, 8);
        assert_eq!(sr, 250);
        assert_eq!(rows.len(), 1);
        assert!((rows[0][1] - 2.0 * SCALE_UV_PER_COUNT).abs() < 1e-9);
    }
}
