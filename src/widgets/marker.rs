//! W_Marker — Software marker widget.
//!
//! Allows sending timestamped markers during a session (very useful for experiments).
//! Current implementation:
//! - Logs marker with high-resolution timestamp via tracing
//! - Shows recent markers in the widget
//! - (Future) Will feed into BDF annotations and networking streams

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct WMarker {
    title: String,
    marker_text: String,
    recent_markers: Vec<(f64, String)>, // (unix timestamp, text)
    max_markers: usize,
}

impl WMarker {
    pub fn new() -> Self {
        Self {
            title: "Marker".to_string(),
            marker_text: String::new(),
            recent_markers: Vec::new(),
            max_markers: 8,
        }
    }

    fn send(&mut self, text: &str, ctx: &mut crate::widget_context::WidgetContext) {
        if text.trim().is_empty() {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        // The context does the heavy lifting: networking.push_marker + recording annotation + last_marker
        ctx.send_marker(timestamp, text);

        // Keep local history for the widget UI (even if no networking/recording active)
        self.recent_markers.push((timestamp, text.to_string()));
        if self.recent_markers.len() > self.max_markers {
            self.recent_markers.remove(0);
        }

        self.marker_text.clear();
    }
}

impl Default for WMarker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WMarker {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, _source: &dyn DataSource) {}

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.marker_text)
                    .desired_width(180.0)
                    .hint_text("Type marker..."),
            );

            if ui.button("Send").clicked()
                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                let text = self.marker_text.clone();
                self.send(&text, ctx);
            }
        });

        ui.add_space(4.0);

        if !self.recent_markers.is_empty() {
            ui.label("Recent markers:");
            for (ts, text) in self.recent_markers.iter().rev() {
                ui.small(format!("{:.1}s: {}", ts, text));
            }
        } else {
            ui.small("No markers sent yet. Use this widget to mark events during recording.");
        }

        let file_marks = source.session_markers();
        if !file_marks.is_empty() {
            ui.separator();
            ui.small("Recording / playback marks (sample index):");
            for m in file_marks.iter().rev().take(8) {
                ui.small(format!("#{}  {:.3}s  {}", m.sample_index, m.board_timestamp, m.label));
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
