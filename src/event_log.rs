//! EventLog — centralized, filterable event log for the GUI.
//!
//! This is the foundation for WConsole (Phase 7).
//! Every important user action, system event, marker, recording start/stop,
//! networking toggle, filter change, connection, and error is logged here with
//! timestamp + category so the user (and future experiment audit) has a complete
//! trace of what happened during a session.
//!
//! Design:
//! - Bounded ring buffer (VecDeque) so it never grows unbounded in a long session.
//! - Categories for filtering in the Console widget: "Marker", "Recording",
//!   "Networking", "Connection", "Focus", "Filter", "System", "Error".
//! - LogLevel for color / severity.
//! - Simple API: log_event(level, category, message) via WidgetContext or direct.
//!
//! plan.md Phase 7 step 1

use std::collections::{HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Marker,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Marker => "MARKER",
        }
    }

    pub fn color(&self) -> egui::Color32 {
        match self {
            LogLevel::Info => egui::Color32::from_rgb(180, 200, 220),
            LogLevel::Warn => egui::Color32::from_rgb(255, 200, 100),
            LogLevel::Error => egui::Color32::from_rgb(255, 100, 100),
            LogLevel::Marker => egui::Color32::from_rgb(100, 200, 255),
        }
    }
}

#[derive(Clone)]
pub struct LogEntry {
    pub timestamp: f64, // seconds since Unix epoch (high-res)
    pub level: LogLevel,
    pub category: String,
    pub message: String,
}

pub struct EventLog {
    entries: VecDeque<LogEntry>,
    max_entries: usize,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLog {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(512),
            max_entries: 2000, // plenty for a full experiment session
        }
    }

    /// Log a new event. Automatically trims old entries when capacity is reached.
    pub fn log(&mut self, level: LogLevel, category: &str, message: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        self.entries.push_back(LogEntry {
            timestamp: ts,
            level,
            category: category.to_string(),
            message: message.to_string(),
        });

        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
    }

    /// Convenience helpers for common categories (used by app and widgets via ctx)
    pub fn log_marker(&mut self, text: &str) {
        self.log(LogLevel::Marker, "Marker", text);
    }

    pub fn log_recording(&mut self, message: &str) {
        self.log(LogLevel::Info, "Recording", message);
    }

    pub fn log_networking(&mut self, message: &str) {
        self.log(LogLevel::Info, "Networking", message);
    }

    pub fn log_connection(&mut self, message: &str) {
        self.log(LogLevel::Info, "Connection", message);
    }

    /// Phase 7: category symmetry for Console filtering (all other cats have dedicated log_*).
    /// Currently widgets use ctx.log_event or the specific helpers; retained for completeness.
    #[allow(dead_code)]
    pub fn log_focus(&mut self, message: &str) {
        self.log(LogLevel::Info, "Focus", message);
    }

    pub fn log_filter(&mut self, message: &str) {
        self.log(LogLevel::Info, "Filter", message);
    }

    pub fn log_error(&mut self, message: &str) {
        self.log(LogLevel::Error, "Error", message);
    }

    pub fn log_system(&mut self, message: &str) {
        self.log(LogLevel::Info, "System", message);
    }

    /// Clear all entries (user action in Console)
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Return a filtered view (most recent first for UI display).
    /// `active_categories` is the set of category strings the user wants to see.
    /// `search` is a case-insensitive substring filter on message or category.
    pub fn filtered(&self, active_categories: &HashSet<String>, search: &str) -> Vec<&LogEntry> {
        let search_lower = search.to_lowercase();
        self.entries
            .iter()
            .rev() // newest on top
            .filter(|e| {
                if !active_categories.is_empty() && !active_categories.contains(&e.category) {
                    return false;
                }
                if search.is_empty() {
                    return true;
                }
                e.message.to_lowercase().contains(&search_lower)
                    || e.category.to_lowercase().contains(&search_lower)
            })
            .collect()
    }

    /// Total number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Phase 7: idiomatic companion to len(); used in Console UI via filtered() but direct call
    /// available for future status checks. Retained.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the most recent N entries (for mini status bar preview).
    pub fn last_n(&self, n: usize) -> Vec<&LogEntry> {
        self.entries.iter().rev().take(n).collect()
    }
}
