//! PROPERTIES card: pairwise slow-wave lag. Not a travel overlay.

use crate::board::DataSource;
use crate::slow_waves::{
    analyze_window, run_self_test, Band, LagResult, SelfTestReport, MONTAGE, WINDOW_SEC,
};
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;

pub struct WSlowWaves {
    title: String,
    band: Band,
    results: Vec<LagResult>,
    self_test: Option<SelfTestReport>,
}

impl WSlowWaves {
    pub fn new() -> Self {
        Self {
            title: "Slow Waves".to_string(),
            band: Band::Slow,
            results: Vec::new(),
            self_test: None,
        }
    }
}

impl Default for WSlowWaves {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WSlowWaves {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let sr = source.sample_rate() as f64;
        if sr <= 1.0 {
            self.results.clear();
            return;
        }
        let n = (WINDOW_SEC * sr).round() as usize;
        let data = source.get_data(n.max(32));
        let exg = source.exg_channels();
        if exg.len() < 8 || data.is_empty() {
            self.results.clear();
            return;
        }
        let mut channels = Vec::with_capacity(8);
        for &col in exg.iter().take(8) {
            channels.push(
                data.iter()
                    .map(|row| row.get(col).copied().unwrap_or(0.0))
                    .collect::<Vec<f64>>(),
            );
        }
        self.results = analyze_window(&channels, sr, self.band);
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        _source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.label(
            egui::RichText::new("Slow waves (pairwise lag, not a path)")
                .small()
                .color(theme::TEXT),
        );
        ui.small(format!(
            "{} · Fp1+Fp2 vs P3+P4, C3 vs C4. Zero lag is volume conduction.",
            MONTAGE.join(" ")
        ));

        ui.horizontal(|ui| {
            ui.small("Band");
            egui::ComboBox::from_id_salt("slow_wave_band")
                .selected_text(self.band.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.band, Band::Slow, Band::Slow.label());
                    ui.selectable_value(&mut self.band, Band::Theta, Band::Theta.label());
                });
        });

        if self.results.is_empty() {
            ui.small("Waiting for 8ch EXG…");
        } else {
            for r in &self.results {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(r.human_copy()).color(theme::TEXT));
                ui.small(format!(
                    "lag {:+.0} ms   conf {:.0}%",
                    r.lag_ms,
                    r.conf * 100.0
                ));
            }
        }

        ui.add_space(6.0);
        if ui.button("Self-test").clicked() {
            self.self_test = Some(run_self_test());
        }
        if let Some(report) = &self.self_test {
            let color = if report.ok() {
                theme::START
            } else {
                theme::STOP
            };
            let head = if report.ok() {
                format!("PASS  {}/{}", report.passed, report.passed + report.failed)
            } else {
                format!("FAIL  {}/{}", report.passed, report.passed + report.failed)
            };
            ui.colored_label(color, head);
            for line in &report.lines {
                ui.small(line);
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
