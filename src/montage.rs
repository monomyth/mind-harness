//! Named 8-channel montage profiles: channel → 10-20 hole.
//! Lives in the app config dir as `montages.json`, not in Session settings.

use crate::widgets::head_plot::LABELS;
use crate::widgets::mark_iv::{hole_index, HEADSET_NAME};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_PROFILE: &str = "8ch 10-20";

fn default_holes() -> [String; 8] {
    LABELS.map(|s| s.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MontageProfile {
    pub name: String,
    /// Channel index 0..7 → 10-20 site name. Empty string = unwired.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Legacy 2D plate labels; used only to migrate old files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xy: Vec<[f32; 2]>,
}

impl MontageProfile {
    pub fn default_10_20() -> Self {
        Self {
            name: DEFAULT_PROFILE.to_string(),
            channels: LABELS.iter().map(|s| s.to_string()).collect(),
            labels: Vec::new(),
            xy: Vec::new(),
        }
    }

    pub fn channel_labels(&self) -> [String; 8] {
        self.channel_holes()
    }

    pub fn channel_holes(&self) -> [String; 8] {
        vec_to_map(&self.channels)
    }

    fn normalize(&mut self) {
        if self.channels.len() != 8 {
            if self.labels.len() == 8 && self.labels.iter().any(|s| hole_index(s).is_some()) {
                self.channels = self.labels.clone();
            } else {
                self.channels = LABELS.iter().map(|s| s.to_string()).collect();
            }
        }
        for slot in self.channels.iter_mut() {
            if !slot.is_empty() && hole_index(slot).is_none() {
                *slot = String::new();
            }
        }
        while self.channels.len() < 8 {
            self.channels.push(String::new());
        }
        self.channels.truncate(8);
        self.xy.clear();
        self.labels.clear();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MontageFile {
    last: String,
    #[serde(default = "default_headset")]
    headset: String,
    profiles: Vec<MontageProfile>,
}

fn default_headset() -> String {
    HEADSET_NAME.to_string()
}

pub struct MontageStore {
    path: Option<PathBuf>,
    last: String,
    headset: String,
    profiles: Vec<MontageProfile>,
}

impl MontageStore {
    pub fn load() -> Self {
        let dir = directories::ProjectDirs::from("com", "openbci", "gui-rust")
            .map(|p| p.config_dir().to_path_buf());
        match dir {
            Some(d) => Self::open(&d),
            None => Self::in_memory(),
        }
    }

    pub fn open(dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("montages.json");
        let mut store = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<MontageFile>(&text) {
                Ok(f) => Self {
                    path: Some(path.clone()),
                    last: f.last,
                    headset: if f.headset.is_empty() {
                        HEADSET_NAME.to_string()
                    } else {
                        f.headset
                    },
                    profiles: f.profiles,
                },
                Err(_) => Self {
                    path: Some(path.clone()),
                    last: DEFAULT_PROFILE.to_string(),
                    headset: HEADSET_NAME.to_string(),
                    profiles: vec![MontageProfile::default_10_20()],
                },
            },
            Err(_) => Self {
                path: Some(path),
                last: DEFAULT_PROFILE.to_string(),
                headset: HEADSET_NAME.to_string(),
                profiles: vec![MontageProfile::default_10_20()],
            },
        };
        for p in store.profiles.iter_mut() {
            p.normalize();
        }
        store.ensure_default();
        if store.find(&store.last).is_none() {
            store.last = DEFAULT_PROFILE.to_string();
        }
        store.persist();
        store
    }

    pub fn in_memory() -> Self {
        let mut s = Self {
            path: None,
            last: DEFAULT_PROFILE.to_string(),
            headset: HEADSET_NAME.to_string(),
            profiles: vec![MontageProfile::default_10_20()],
        };
        s.ensure_default();
        s
    }

    fn ensure_default(&mut self) {
        const LEGACY_KIT: [&str; 8] = ["Fp1", "Fp2", "F7", "F8", "C3", "C4", "P3", "P4"];
        match self.find(DEFAULT_PROFILE) {
            None => self.profiles.insert(0, MontageProfile::default_10_20()),
            Some(i) => {
                let cur: Vec<&str> = self.profiles[i].channels.iter().map(|s| s.as_str()).collect();
                if cur == LEGACY_KIT {
                    self.profiles[i].channels = LABELS.iter().map(|s| s.to_string()).collect();
                }
            }
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.profiles.iter().position(|p| p.name == name)
    }

    pub fn names(&self) -> Vec<String> {
        self.profiles.iter().map(|p| p.name.clone()).collect()
    }

    pub fn last_name(&self) -> &str {
        &self.last
    }

    pub fn headset(&self) -> &str {
        &self.headset
    }

    pub fn active(&self) -> &MontageProfile {
        let i = self.find(&self.last).unwrap_or(0);
        &self.profiles[i]
    }

    pub fn active_map(&self) -> [String; 8] {
        self.active().channel_holes()
    }

    pub fn select(&mut self, name: &str) -> bool {
        if self.find(name).is_none() {
            return false;
        }
        self.last = name.to_string();
        self.persist();
        true
    }

    /// Write current channel→site into the active profile (update in place).
    pub fn save_active(&mut self, holes: [String; 8]) {
        let i = self.find(&self.last).unwrap_or(0);
        self.profiles[i].channels = holes.to_vec();
        self.profiles[i].normalize();
        self.persist();
    }

    /// Create or replace a named profile with current map, then make it last-used.
    pub fn save_as(&mut self, name: &str, holes: [String; 8]) -> bool {
        let name = sanitize_name(name);
        if name.is_empty() {
            return false;
        }
        let mut profile = MontageProfile {
            name: name.clone(),
            channels: holes.to_vec(),
            labels: Vec::new(),
            xy: Vec::new(),
        };
        profile.normalize();
        if let Some(i) = self.find(&name) {
            self.profiles[i] = profile;
        } else {
            self.profiles.push(profile);
        }
        self.last = name;
        self.persist();
        true
    }

    fn persist(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let file = MontageFile {
            last: self.last.clone(),
            headset: self.headset.clone(),
            profiles: self.profiles.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&file) {
            let _ = std::fs::write(path, json);
        }
    }
}

pub fn sanitize_name(name: &str) -> String {
    name.trim()
        .replace(['/', '\\', ':', '\n', '\r'], "")
        .chars()
        .take(48)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn vec_to_map(v: &[String]) -> [String; 8] {
    let mut out = default_holes();
    for (i, s) in v.iter().take(8).enumerate() {
        out[i] = s.clone();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("obci-montage-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn default_round_trip() {
        let dir = scratch();
        let store = MontageStore::open(&dir);
        assert_eq!(store.last_name(), DEFAULT_PROFILE);
        assert_eq!(store.active().channel_holes(), default_holes());
        assert_eq!(store.headset(), HEADSET_NAME);
        drop(store);
        let again = MontageStore::open(&dir);
        assert_eq!(again.last_name(), DEFAULT_PROFILE);
        assert_eq!(again.active().channel_holes(), default_holes());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_profile_drop_app_load_same_holes() {
        let dir = scratch();
        let mut moved = default_holes();
        moved[2] = "T7".into();
        moved[6] = "P3".into();
        {
            let mut store = MontageStore::open(&dir);
            assert!(store.save_as("Eugene cap", moved.clone()));
            assert_eq!(store.last_name(), "Eugene cap");
            assert_eq!(store.active().channel_holes()[2], "T7");
        }
        let store = MontageStore::open(&dir);
        assert_eq!(store.last_name(), "Eugene cap");
        assert_eq!(store.active().channel_holes()[2], "T7");
        assert_eq!(store.active().channel_holes()[6], "P3");
        assert_eq!(store.active().channel_holes()[0], "Fp1");
        let json = std::fs::read_to_string(dir.join("montages.json")).unwrap();
        assert!(json.contains("\"T7\""));
        assert!(!json.contains("\"xy\""));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn switch_profile() {
        let dir = scratch();
        let mut a = default_holes();
        a[0] = "Fz".into();
        let mut b = default_holes();
        b[1] = "O2".into();
        let mut store = MontageStore::open(&dir);
        store.save_as("cap A", a);
        store.save_as("cap B", b);
        assert_eq!(store.last_name(), "cap B");
        assert_eq!(store.active().channel_holes()[1], "O2");
        assert!(store.select("cap A"));
        assert_eq!(store.last_name(), "cap A");
        assert_eq!(store.active().channel_holes()[0], "Fz");
        assert_eq!(store.active().channel_holes()[1], "Fp2");
        drop(store);
        let store = MontageStore::open(&dir);
        assert_eq!(store.last_name(), "cap A");
        assert_eq!(store.active().channel_holes()[0], "Fz");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_active_updates_in_place() {
        let dir = scratch();
        let mut store = MontageStore::open(&dir);
        let mut map = default_holes();
        map[3] = "F4".into();
        store.save_active(map);
        assert_eq!(store.last_name(), DEFAULT_PROFILE);
        drop(store);
        let store = MontageStore::open(&dir);
        assert_eq!(store.active().channel_holes()[3], "F4");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn migrate_legacy_labels_ignore_xy() {
        let dir = scratch();
        let json = r#"{
            "last": "8ch 10-20",
            "profiles": [{
                "name": "8ch 10-20",
                "labels": ["Fp1","Fp2","F7","F8","C3","C4","P3","P4"],
                "xy": [[-0.28,-0.52],[0.28,-0.52],[-0.58,-0.08],[0.58,-0.08],[-0.42,0.22],[0.42,0.22],[-0.22,0.58],[0.22,0.58]]
            }]
        }"#;
        std::fs::write(dir.join("montages.json"), json).unwrap();
        let store = MontageStore::open(&dir);
        // Named "8ch 10-20" is the stock profile and is rewritten to the kit map.
        assert_eq!(store.active().channel_holes(), default_holes());
        assert_eq!(store.active().channel_holes()[2], "C3");
        assert_eq!(store.active().channel_holes()[6], "O1");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn custom_profile_keeps_p3_p4() {
        let dir = scratch();
        let json = r#"{
            "last": "Eugene cap",
            "profiles": [{
                "name": "Eugene cap",
                "channels": ["Fp1","Fp2","F7","F8","C3","C4","P3","P4"]
            }]
        }"#;
        std::fs::write(dir.join("montages.json"), json).unwrap();
        let store = MontageStore::open(&dir);
        assert_eq!(store.last_name(), "Eugene cap");
        assert_eq!(store.active().channel_holes()[2], "F7");
        assert_eq!(store.active().channel_holes()[6], "P3");
        assert_eq!(store.active().channel_holes()[7], "P4");
        let kit = store.profiles.iter().find(|p| p.name == DEFAULT_PROFILE).unwrap();
        assert_eq!(kit.channel_holes()[6], "O1");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn default_is_mark_iv_cyton_8ch() {
        let m = default_holes();
        assert_eq!(m, LABELS.map(|s| s.to_string()));
        assert_eq!(m[2], "C3");
        assert_eq!(m[4], "P7");
        assert_eq!(m[6], "O1");
        assert_eq!(m[7], "O2");
        assert_ne!(m[2], "F7");
        assert_ne!(m[6], "P3");
        assert!(hole_index("P3").is_some());
        assert!(hole_index("F7").is_some());
    }
}
