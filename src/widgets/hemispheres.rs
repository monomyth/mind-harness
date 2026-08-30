//! PROPERTIES card: pair-named laterality + relative power. Not a path on the scalp.

use crate::board::DataSource;
use crate::laterality::{
    pair_band_snapshot, run_self_test, wave_presence, FlipClock, PairBandLi, SelfTestReport,
    WaveRow, GAMMA_NOTICE, WINDOW_SEC,
};
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use std::time::Instant;

pub struct WHemispheres {
    title: String,
    clock: FlipClock,
    rows: Vec<PairBandLi>,
    waves: [WaveRow; 4],
    t0: Instant,
    self_test: Option<SelfTestReport>,
}

impl WHemispheres {
    pub fn new() -> Self {
        Self {
            title: "Hemispheres".to_string(),
            clock: FlipClock::new(),
            rows: Vec::new(),
            waves: wave_presence(&[], 250.0),
            t0: Instant::now(),
            self_test: None,
        }
    }
}

impl Default for WHemispheres {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WHemispheres {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let sr = source.sample_rate() as f64;
        if sr <= 1.0 {
            self.rows.clear();
            return;
        }
        let n = (WINDOW_SEC * sr).round() as usize;
        let data = source.get_data(n.max(32));
        let exg = source.exg_channels();
        if exg.len() < 8 || data.is_empty() {
            self.rows.clear();
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
        self.rows = pair_band_snapshot(&channels, sr);
        self.waves = wave_presence(&channels, sr);
        self.clock
            .observe_rows(&self.rows, self.t0.elapsed().as_secs_f64());
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        _source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.label(
            egui::RichText::new("Power ratio on named pairs. Not a hemisphere state.")
                .small()
                .color(theme::TEXT),
        );
        ui.small("C3/C4 and P3/P4 (no O1/O2). α drop on C3/C4 is ERD, not valence.");

        if self.rows.is_empty() {
            ui.small("Waiting for 8ch EXG…");
        } else {
            for r in &self.rows {
                ui.horizontal(|ui| {
                    paint_li_bar(ui, r.li);
                    ui.small(egui::RichText::new(r.row_copy()).color(theme::TEXT));
                });
            }
        }

        ui.add_space(4.0);
        ui.small(GAMMA_NOTICE);
        ui.label(
            egui::RichText::new(self.clock.last_flip_line(self.t0.elapsed().as_secs_f64()))
                .small()
                .color(theme::TEXT),
        );

        ui.add_space(6.0);
        ui.small("Relative spectral power on these electrodes, not a diagnosis.");
        for w in &self.waves {
            ui.small(w.row_copy());
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

fn paint_li_bar(ui: &mut egui::Ui, li: Option<f64>) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(64.0, 10.0), egui::Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, theme::TRANSPORT);
    painter.rect_stroke(r, 2.0, theme::hairline(), egui::StrokeKind::Inside);
    let mid = r.center().x;
    painter.line_segment(
        [
            egui::pos2(mid, r.top() + 1.0),
            egui::pos2(mid, r.bottom() - 1.0),
        ],
        theme::hairline(),
    );
    if let Some(v) = li {
        let half = r.width() * 0.5;
        let x = mid - v.clamp(-1.0, 1.0) as f32 * half;
        let fill = if v >= 0.0 {
            egui::Rect::from_min_max(
                egui::pos2(x, r.top() + 2.0),
                egui::pos2(mid, r.bottom() - 2.0),
            )
        } else {
            egui::Rect::from_min_max(
                egui::pos2(mid, r.top() + 2.0),
                egui::pos2(x, r.bottom() - 2.0),
            )
        };
        painter.rect_filled(fill, 0.0, theme::ACCENT);
    }
}
