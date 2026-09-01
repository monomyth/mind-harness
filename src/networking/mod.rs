//! Networking output module.
//!
//! This is the Rust equivalent of the original OpenBCI GUI's Networking widget
//! (W_Networking.pde + NetworkStreamOut.pde).
//!
//! Supported protocols:
//! - LSL (Lab Streaming Layer) - primary research use
//! - UDP (plain text or JSON)
//! - OSC (Open Sound Control)

pub mod lsl_stream;
pub mod osc;
pub mod udp_stream;

use crate::board::{self, DataSource};

/// Configuration for one networking output stream.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StreamConfig {
    pub protocol: Protocol,
    pub enabled: bool,
    pub target: String, // e.g. "localhost:12345" or LSL stream name
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum Protocol {
    LSL,
    UDP,
    OSC,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::LSL,
            enabled: false,
            target: crate::networking::lsl_stream::DEFAULT_EEG_NAME.to_string(),
        }
    }
}

/// The main Networking manager. Holds active output streams.
pub struct NetworkingManager {
    pub configs: Vec<StreamConfig>,
    udp_sender: Option<udp_stream::UdpSender>,
    osc_sender: Option<osc::OscSender>,
    lsl: Option<lsl_stream::LslPair>,
    /// Last apply/send error, shown in the Networking widget (never silently green).
    pub last_error: Option<String>,
    lsl_channel_count: usize,
    lsl_sample_rate: f64,
}

impl NetworkingManager {
    pub fn new() -> Self {
        Self {
            configs: vec![
                StreamConfig {
                    protocol: Protocol::LSL,
                    enabled: false,
                    target: lsl_stream::DEFAULT_EEG_NAME.to_string(),
                },
                StreamConfig {
                    protocol: Protocol::UDP,
                    enabled: false,
                    target: "127.0.0.1:12345".to_string(),
                },
                StreamConfig {
                    protocol: Protocol::OSC,
                    enabled: false,
                    target: "127.0.0.1:9000".to_string(),
                },
            ],
            udp_sender: None,
            osc_sender: None,
            lsl: None,
            last_error: None,
            lsl_channel_count: 8,
            lsl_sample_rate: 250.0,
        }
    }

    pub fn set_lsl_geometry(&mut self, n_ch: usize, sample_rate: f64) {
        self.lsl_channel_count = n_ch.max(1);
        self.lsl_sample_rate = sample_rate.max(1.0);
    }

    /// Called every time new data arrives from the board.
    /// We push samples to all enabled streams.
    pub fn push_data(&mut self, source: &dyn DataSource) {
        if !self.has_active_streams() {
            return;
        }

        let recent = board::recent_rows(source);
        if recent.is_empty() {
            return;
        }
        let exg = source.exg_channels();

        for sample in recent {
            let eeg = board::extract_exg(&sample, exg);
            if let Some(ref mut sender) = self.udp_sender {
                let line = eeg
                    .iter()
                    .map(|v| format!("{:.4}", v))
                    .collect::<Vec<_>>()
                    .join(",");
                if let Err(e) = sender.send(&format!("{}\n", line)) {
                    self.last_error = Some(format!("UDP send failed: {}", e));
                }
            }

            if let Some(ref mut sender) = self.osc_sender {
                if let Err(e) = sender.send_eeg_sample(&eeg) {
                    self.last_error = Some(format!("OSC send failed: {}", e));
                }
            }
            if let Some(ref lsl) = self.lsl {
                if let Err(e) = lsl.push_eeg(&eeg) {
                    self.last_error = Some(format!("LSL send failed: {}", e));
                }
            }
        }
    }

    pub fn has_active_streams(&self) -> bool {
        self.udp_sender.is_some() || self.osc_sender.is_some() || self.lsl.is_some()
    }

    pub fn lsl_connected(&self) -> bool {
        self.lsl.is_some()
    }

    pub fn lsl_available() -> bool {
        lsl_stream::lsl_linked()
    }

    /// Start/stop streams based on current configs.
    pub fn apply_config(&mut self) {
        self.last_error = None;

        let udp_config = self.configs.iter().find(|c| c.protocol == Protocol::UDP);
        let udp_enabled = udp_config.is_some_and(|c| c.enabled);
        if udp_enabled && self.udp_sender.is_none() {
            let target = udp_config
                .map(|c| c.target.as_str())
                .unwrap_or("127.0.0.1:12345");
            match udp_stream::UdpSender::new(target) {
                Ok(sender) => {
                    self.udp_sender = Some(sender);
                    tracing::info!("UDP sender started to {}", target);
                }
                Err(e) => {
                    self.last_error = Some(format!("UDP {} — {}", target, e));
                    tracing::error!("UDP sender failed for {}: {}", target, e);
                }
            }
        } else if !udp_enabled {
            self.udp_sender = None;
        }

        let osc_config = self.configs.iter().find(|c| c.protocol == Protocol::OSC);
        let osc_enabled = osc_config.is_some_and(|c| c.enabled);
        if osc_enabled && self.osc_sender.is_none() {
            let target = osc_config
                .map(|c| c.target.as_str())
                .unwrap_or("127.0.0.1:9000");
            match osc::OscSender::new(target) {
                Ok(sender) => {
                    self.osc_sender = Some(sender);
                    tracing::info!("OSC sender started to {}", target);
                }
                Err(e) => {
                    self.last_error = Some(format!("OSC {} — {}", target, e));
                    tracing::error!("OSC sender failed for {}: {}", target, e);
                }
            }
        } else if !osc_enabled {
            self.osc_sender = None;
        }

        let lsl_config = self.configs.iter().find(|c| c.protocol == Protocol::LSL);
        let lsl_enabled = lsl_config.is_some_and(|c| c.enabled);
        if lsl_enabled && self.lsl.is_none() {
            if !lsl_stream::lsl_linked() {
                self.last_error = Some(
                    "LSL is not linked in this binary (Homebrew lsl.framework missing at build)"
                        .into(),
                );
                if let Some(c) = self
                    .configs
                    .iter_mut()
                    .find(|c| c.protocol == Protocol::LSL)
                {
                    c.enabled = false;
                }
            } else {
                let name = lsl_config
                    .map(|c| c.target.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(lsl_stream::DEFAULT_EEG_NAME);
                match lsl_stream::LslPair::start(name, self.lsl_channel_count, self.lsl_sample_rate)
                {
                    Ok(pair) => {
                        self.lsl = Some(pair);
                        tracing::info!("LSL outlet {name} (EEG + Markers)");
                    }
                    Err(e) => {
                        self.last_error = Some(format!("LSL {name} — {e}"));
                        if let Some(c) = self
                            .configs
                            .iter_mut()
                            .find(|c| c.protocol == Protocol::LSL)
                        {
                            c.enabled = false;
                        }
                    }
                }
            }
        } else if !lsl_enabled {
            self.lsl = None;
        }
    }

    pub fn udp_connected(&self) -> bool {
        self.udp_sender.is_some()
    }

    pub fn osc_connected(&self) -> bool {
        self.osc_sender.is_some()
    }

    pub fn config_mut(&mut self, protocol: Protocol) -> Option<&mut StreamConfig> {
        self.configs.iter_mut().find(|c| c.protocol == protocol)
    }

    pub fn stop_all(&mut self) {
        self.udp_sender = None;
        self.osc_sender = None;
        self.lsl = None;
        for c in &mut self.configs {
            c.enabled = false;
        }
    }

    /// Update a specific stream config (used by the Networking widget)
    pub fn update_config(&mut self, protocol: Protocol, enabled: bool, target: &str) {
        if let Some(config) = self.configs.iter_mut().find(|c| c.protocol == protocol) {
            config.enabled = enabled;
            config.target = target.to_string();
        }
    }

    /// Send a marker to all active streams (for experiment synchronization)
    pub fn push_marker(&mut self, timestamp: f64, marker: &str) {
        if let Some(ref mut sender) = self.udp_sender {
            let _ = sender.send(&format!("MARKER,{:.3},{}\n", timestamp, marker));
        }
        if let Some(ref mut sender) = self.osc_sender {
            let _ = sender.send_marker(timestamp, marker);
        }
        if let Some(ref lsl) = self.lsl {
            let _ = lsl.push_marker(timestamp, marker);
        }
    }
}
