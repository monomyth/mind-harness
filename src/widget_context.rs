//! WidgetContext — enables interactive widgets to safely affect shared application state.
//!
//! This is the critical foundation for Phase 4 (and future interactive widgets like Console).
//! Widgets receive a mutable context during their `show()` call and can:
//! - Send markers (which flow to NetworkingManager + RecordPump annotations + status bar)
//! - Update Networking targets / enabled state and apply changes
//! - Log to the central EventLog (powers WConsole — Phase 7)
//!
//! Design: direct &mut references (safe because egui is single-threaded immediate mode).
//!
//! plan.md Phase 7 step 1 — EventLog integration

use crate::data_logger::RecordPump;
use crate::emg::EmgProcessor;
use crate::event_log::{EventLog, LogLevel};
use crate::networking::{NetworkingManager, Protocol};

pub struct WidgetContext<'a> {
    pub networking: &'a mut NetworkingManager,
    pub data_logger: &'a mut RecordPump,
    pub last_marker: &'a mut String,
    /// Central event log (WConsole reads from this). Never None in a live session.
    pub event_log: &'a mut EventLog,
    /// Java `dataProcessing.emgSettings.values` — shared by EMG + EMG Joystick.
    pub emg: &'a mut EmgProcessor,
}

impl<'a> WidgetContext<'a> {
    pub fn new(
        networking: &'a mut NetworkingManager,
        data_logger: &'a mut RecordPump,
        last_marker: &'a mut String,
        event_log: &'a mut EventLog,
        emg: &'a mut EmgProcessor,
    ) -> Self {
        Self {
            networking,
            data_logger,
            last_marker,
            event_log,
            emg,
        }
    }

    /// Send a timestamped marker.
    /// - Pushes to all active UDP/OSC streams (MARKER,ts,text CSV or OSC /marker)
    /// - If recording, writes a BDF annotation
    /// - Updates the bottom status bar "Last Marker: ..."
    /// - Logs to EventLog (visible in WConsole)
    pub fn send_marker(&mut self, timestamp: f64, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.networking.push_marker(timestamp, text);
        if self.data_logger.is_logging() {
            let _ = self.data_logger.write_marker_annotation(timestamp, text);
        }
        *self.last_marker = text.to_string();
        tracing::info!("[MARKER] {:.3} - {} (via context)", timestamp, text);

        // Phase 7: also record in the central audit log
        self.event_log.log_marker(text);
    }

    /// Apply current networking configs (creates senders for enabled protocols).
    pub fn apply_networking(&mut self) {
        self.networking.apply_config();
        self.event_log
            .log_networking("Networking configuration applied");
    }

    /// Update one protocol's config (used by WNetworking UI when user edits targets).
    pub fn update_networking_config(&mut self, protocol: Protocol, enabled: bool, target: &str) {
        self.networking.update_config(protocol, enabled, target);
        self.event_log.log_networking(&format!(
            "{} {} → {}",
            if enabled { "Enabled" } else { "Disabled" },
            match protocol {
                Protocol::UDP => "UDP",
                Protocol::OSC => "OSC",
                Protocol::LSL => "LSL",
            },
            target
        ));
    }

    /// Log any event from a widget or the app (used heavily by WConsole, Focus, etc.)
    pub fn log_event(&mut self, level: LogLevel, category: &str, message: &str) {
        self.event_log.log(level, category, message);
        match level {
            LogLevel::Error => tracing::error!("[{}] {}", category, message),
            LogLevel::Warn => tracing::warn!("[{}] {}", category, message),
            _ => tracing::info!("[{}] {}", category, message),
        }
    }
}
