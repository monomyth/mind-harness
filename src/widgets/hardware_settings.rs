//! Board (ADS1299) — Cyton ADS1299 power / gain / input / bias / SRB.
//!
//! Pending commits are polled by the app (same pattern as WImpedance).

use crate::board::ads_settings::{AdsChannel, AdsGain, AdsInput, AdsPower, AdsYesNo};
use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;

pub struct WHardwareSettings {
    title: String,
    pending: Option<(usize, AdsChannel)>,
}

impl WHardwareSettings {
    pub fn new() -> Self {
        Self {
            title: "Board".to_string(),
            pending: None,
        }
    }

    pub fn take_pending(&mut self) -> Option<(usize, AdsChannel)> {
        self.pending.take()
    }
}

impl Default for WHardwareSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WHardwareSettings {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, _source: &dyn DataSource) {}

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        let Some(bank) = source.ads_channels() else {
            ui.colored_label(
                egui::Color32::from_rgb(200, 160, 80),
                "ADS1299 channel commands are for Cyton / Synthetic only",
            );
            return;
        };
        ui.small(
            "Power Off zeros that electrode in Time Series. Live Cyton sends x…X via config_board.",
        );
        egui::ScrollArea::both().max_height(280.0).show(ui, |ui| {
            egui::Grid::new("ads_grid").striped(true).show(ui, |ui| {
                ui.strong("Ch");
                ui.strong("Pwr");
                ui.strong("Gain");
                ui.strong("Input");
                ui.strong("Bias");
                ui.strong("SRB2");
                ui.end_row();
                for (i, ch) in bank.iter().enumerate() {
                    let mut next = *ch;
                    ui.label(format!("{}", i + 1));
                    let mut on = next.power == AdsPower::On;
                    if ui.checkbox(&mut on, "").changed() {
                        next.power = if on { AdsPower::On } else { AdsPower::Off };
                    }
                    egui::ComboBox::from_id_salt(format!("gain{i}"))
                        .selected_text(gain_label(next.gain))
                        .width(52.0)
                        .show_ui(ui, |ui| {
                            for g in [
                                AdsGain::X1,
                                AdsGain::X2,
                                AdsGain::X4,
                                AdsGain::X6,
                                AdsGain::X8,
                                AdsGain::X12,
                                AdsGain::X24,
                            ] {
                                ui.selectable_value(&mut next.gain, g, gain_label(g));
                            }
                        });
                    egui::ComboBox::from_id_salt(format!("in{i}"))
                        .selected_text(input_label(next.input))
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            for inp in [AdsInput::Normal, AdsInput::Shorted, AdsInput::Test] {
                                ui.selectable_value(&mut next.input, inp, input_label(inp));
                            }
                        });
                    let mut bias = next.bias == AdsYesNo::Yes;
                    if ui.checkbox(&mut bias, "").changed() {
                        next.bias = if bias { AdsYesNo::Yes } else { AdsYesNo::No };
                    }
                    let mut srb2 = next.srb2 == AdsYesNo::Yes;
                    if ui.checkbox(&mut srb2, "").changed() {
                        next.srb2 = if srb2 { AdsYesNo::Yes } else { AdsYesNo::No };
                    }
                    if next != *ch {
                        self.pending = Some((i, next));
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn gain_label(g: AdsGain) -> &'static str {
    match g {
        AdsGain::X1 => "x1",
        AdsGain::X2 => "x2",
        AdsGain::X4 => "x4",
        AdsGain::X6 => "x6",
        AdsGain::X8 => "x8",
        AdsGain::X12 => "x12",
        AdsGain::X24 => "x24",
    }
}

fn input_label(i: AdsInput) -> &'static str {
    match i {
        AdsInput::Normal => "Normal",
        AdsInput::Shorted => "Short",
        AdsInput::BiasMeas => "BiasM",
        AdsInput::Mvdd => "MVDD",
        AdsInput::Temp => "Temp",
        AdsInput::Test => "Test",
        AdsInput::BiasDrp => "BiasP",
        AdsInput::BiasDrn => "BiasN",
    }
}
