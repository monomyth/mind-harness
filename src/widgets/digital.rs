//! W_DigitalRead — Cyton digital pins (Java D11/D12/D13/D17/D18). Hidden on Synthetic.

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;

pub struct WDigitalRead {
    title: String,
    last: Vec<f64>,
    pending_mode: Option<u8>,
}

impl WDigitalRead {
    pub fn new() -> Self {
        Self {
            title: "Digital Read".to_string(),
            last: vec![],
            pending_mode: None,
        }
    }
    pub fn take_pending_mode(&mut self) -> Option<u8> {
        self.pending_mode.take()
    }
}

impl Default for WDigitalRead {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WDigitalRead {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let chans = source.digital_channels();
        if chans.is_empty() || !source.supports_aux_widgets() {
            return;
        }
        if let Some(row) = source.get_raw_data(1).last() {
            self.last = chans
                .iter()
                .map(|&c| row.get(c).copied().unwrap_or(0.0))
                .collect();
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        if !source.supports_aux_widgets() {
            ui.label("no aux — Digital Read is Cyton-only");
            return;
        }
        if source.digital_channels().is_empty() {
            ui.label("no aux — this board has no digital channels");
            return;
        }
        ui.horizontal(|ui| {
            if source.cyton_board_mode() != Some(3) {
                if ui.button("Turn Digital Read On").clicked() {
                    self.pending_mode = Some(3);
                }
                ui.small("sends /3 (Java CytonBoardMode.DIGITAL)");
            } else if ui.button("Default mode").clicked() {
                self.pending_mode = Some(0);
            }
        });
        let labels = ["D11", "D12", "D13", "D17", "D18"];
        ui.horizontal(|ui| {
            for (i, &lab) in labels.iter().enumerate() {
                let on = self.last.get(i).copied().unwrap_or(0.0) > 0.5;
                let col = if on {
                    egui::Color32::from_rgb(80, 200, 120)
                } else {
                    egui::Color32::from_gray(140)
                };
                ui.colored_label(col, format!("{lab} {}", if on { "HIGH" } else { "LOW" }));
            }
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
