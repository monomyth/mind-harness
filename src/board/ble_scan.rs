//! Ganglion BLE discovery (macOS).
//!
//! BrainFlow's Rust binding has no scanner. We list devices whose name or
//! address looks like a Ganglion from `system_profiler SPBluetoothDataType`.
//! An empty result is "none found", not a synthetic fallback.

use serde_json::Value;
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GanglionDevice {
    pub name: String,
    pub id: String,
}

fn looks_like_ganglion(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("ganglion")
}

fn push_device(out: &mut Vec<GanglionDevice>, name: &str, id: &str) {
    if !looks_like_ganglion(name) && !looks_like_ganglion(id) {
        return;
    }
    let id = if id.is_empty() {
        name.to_string()
    } else {
        id.to_string()
    };
    if out.iter().any(|d| d.id == id || d.name == name) {
        return;
    }
    out.push(GanglionDevice {
        name: if name.is_empty() {
            id.clone()
        } else {
            name.to_string()
        },
        id,
    });
}

fn walk_json(v: &Value, out: &mut Vec<GanglionDevice>) {
    match v {
        Value::Object(map) => {
            let name = map
                .get("device_name")
                .or_else(|| map.get("_name"))
                .or_else(|| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let addr = map
                .get("device_address")
                .or_else(|| map.get("device_addr"))
                .or_else(|| map.get("addr"))
                .or_else(|| map.get("MAC Address"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !name.is_empty() || !addr.is_empty() {
                push_device(out, name, addr);
            }
            for (k, child) in map {
                if looks_like_ganglion(k) {
                    push_device(out, k, addr);
                }
                walk_json(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk_json(child, out);
            }
        }
        Value::String(s) if looks_like_ganglion(s) => push_device(out, s, s),
        _ => {}
    }
}

/// Parse `system_profiler -json` output.
pub fn devices_from_profiler_json(json: &str) -> Vec<GanglionDevice> {
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(json) {
        walk_json(&v, &mut out);
    }
    out
}

/// Scan for Ganglion names/MACs. Empty list means none found (fail closed).
pub fn scan_ganglions(_timeout: Duration) -> Result<Vec<GanglionDevice>, String> {
    let output = Command::new("system_profiler")
        .arg("SPBluetoothDataType")
        .arg("-json")
        .output()
        .map_err(|e| format!("Bluetooth scan failed: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Bluetooth scan failed: {err}"));
    }
    let json = String::from_utf8_lossy(&output.stdout);
    Ok(devices_from_profiler_json(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ganglion_from_profiler_json() {
        let json = r#"{
            "SPBluetoothDataType": [{
                "device_connected": [{
                    "device_name": "Ganglion-1a2b",
                    "device_address": "AA:BB:CC:DD:EE:FF"
                }]
            }]
        }"#;
        let d = devices_from_profiler_json(json);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "Ganglion-1a2b");
        assert_eq!(d[0].id, "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn ignores_unrelated_devices() {
        let json =
            r#"{"SPBluetoothDataType":[{"device_name":"AirPods","device_address":"11:22"}]}"#;
        assert!(devices_from_profiler_json(json).is_empty());
    }
}
