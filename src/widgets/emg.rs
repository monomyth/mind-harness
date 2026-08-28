//! W_EMG — per-channel EMG envelope circles + normalized bars (Java `W_emg`).

use crate::board::DataSource;
use crate::emg::{
    EmgProcessor, EmgUvLimit, EmgWindow, LOWER_MIN_UV, MIN_DELTA_UV,
};
use crate::theme;
use crate::widgets::Widget;
use eframe::egui;
use egui::{pos2, Color32, Rect, Stroke};

pub struct WEmg {
    title: String,
    visible: Vec<bool>,
    show_settings: bool,
}

impl WEmg {
    pub fn new() -> Self {
        Self {
            title: "EMG".to_string(),
            visible: vec![true; 16],
            show_settings: false,
        }
    }
}

impl Default for WEmg {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WEmg {
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
        let n = source.exg_channels().len().min(ctx.emg.channels.len());
        if self.visible.len() < n {
            self.visible.resize(n, true);
        }

        ui.horizontal(|ui| {
            if ui.button("EMG Settings").clicked() {
                self.show_settings = !self.show_settings;
            }
            ui.small("Circles: envelope vs thresholds. Bar: 0–1 mapped output.");
        });

        if self.show_settings {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                emg_settings_ui(ui, ctx.emg);
            });
        }

        let active: Vec<usize> = (0..n).filter(|&i| self.visible[i]).collect();
        if active.is_empty() {
            ui.small("All channels off.");
            channel_toggles(ui, n, &mut self.visible);
            return;
        }

        let row_count = 4usize;
        let col_count = active.len().div_ceil(row_count).max(1);
        let avail = ui.available_size();
        let cell_w = (avail.x / col_count as f32).max(48.0);
        let cell_h = ((avail.y - 22.0) / row_count as f32).max(48.0);

        for r in 0..row_count {
            ui.horizontal(|ui| {
                for c in 0..col_count {
                    let idx = r * col_count + c;
                    let Some(&ch) = active.get(idx) else {
                        ui.allocate_exact_size(egui::vec2(cell_w, cell_h), egui::Sense::hover());
                        continue;
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(cell_w, cell_h), egui::Sense::hover());
                    if let Some(state) = ctx.emg.channels.get(ch) {
                        paint_emg_cell(ui, rect, ch, state);
                    }
                }
            });
        }

        channel_toggles(ui, n, &mut self.visible);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn channel_toggles(ui: &mut egui::Ui, n: usize, visible: &mut [bool]) {
    ui.horizontal_wrapped(|ui| {
        ui.small("Ch");
        for i in 0..n {
            let mut on = visible.get(i).copied().unwrap_or(true);
            if ui.selectable_label(on, format!("{}", i + 1)).clicked() {
                on = !on;
                if i < visible.len() {
                    visible[i] = on;
                }
            }
        }
    });
}

pub fn paint_emg_cell(ui: &egui::Ui, rect: Rect, channel: usize, state: &crate::emg::EmgChannelState) {
    let painter = ui.painter_at(rect);
    let color = theme::channel_color(channel);
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 200);
    let stroke = Stroke::new(
        1.0_f32,
        Color32::from_rgba_unmultiplied(
            theme::OPENBCI_DARKBLUE.r(),
            theme::OPENBCI_DARKBLUE.g(),
            theme::OPENBCI_DARKBLUE.b(),
            150,
        ),
    );

    let limit = state.settings.uv_limit.uv().max(1.0);
    let max_d = rect.height().min(rect.width() * 0.45) * 0.85;
    let to_r = |uv: f64| ((uv / limit) as f32 * max_d * 0.5).clamp(1.0, max_d * 0.5);

    let circ = pos2(rect.left() + rect.width() * 0.28, rect.center().y);
    painter.circle_filled(circ, to_r(state.average_uv), fill);
    painter.circle_stroke(circ, to_r(state.upper_threshold), stroke);
    painter.circle_stroke(circ, to_r(state.lower_threshold), stroke);

    painter.text(
        pos2(rect.left() + 6.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        format!("{}", channel + 1),
        egui::FontId::proportional(12.0),
        theme::OPENBCI_DARKBLUE,
    );

    let bar_w = (rect.width() * 0.16).clamp(8.0, 18.0);
    let bar_h = rect.height() * 0.5;
    let bar_left = rect.left() + rect.width() * 0.62;
    let bar_top = rect.center().y - bar_h * 0.5;
    let bar_bg = Rect::from_min_size(pos2(bar_left, bar_top), egui::vec2(bar_w, bar_h));
    painter.rect_stroke(bar_bg, 0.0, stroke, egui::StrokeKind::Inside);
    let n = state.output_normalized.clamp(0.0, 1.0) as f32;
    let fill_h = bar_h * n;
    if fill_h > 0.5 {
        painter.rect_filled(
            Rect::from_min_max(
                pos2(bar_bg.left(), bar_bg.bottom() - fill_h),
                pos2(bar_bg.right(), bar_bg.bottom()),
            ),
            0.0,
            fill,
        );
    }
}

pub fn emg_settings_ui(ui: &mut egui::Ui, emg: &mut EmgProcessor) {
    ui.small("Per-channel envelope (Java EMG Settings).");
    if ui.small_button("Reset defaults").clicked() {
        for ch in &mut emg.channels {
            ch.settings = crate::emg::EmgChannelSettings::default();
        }
    }
    egui::ScrollArea::horizontal().show(ui, |ui| {
        egui::Grid::new("emg_settings_grid")
            .striped(true)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.small("Ch");
                ui.small("Window");
                ui.small("uV limit");
                ui.small("Creep ↑");
                ui.small("Creep ↓");
                ui.small("Min Δ");
                ui.small("Low min");
                ui.end_row();
                for (i, ch) in emg.channels.iter_mut().enumerate() {
                    ui.small(format!("{}", i + 1));
                    combo_window(ui, i, &mut ch.settings.window);
                    combo_limit(ui, i, &mut ch.settings.uv_limit);
                    combo_creep(ui, format!("ci{i}"), &mut ch.settings.creep_increasing);
                    combo_creep(ui, format!("cd{i}"), &mut ch.settings.creep_decreasing);
                    combo_f64(ui, format!("md{i}"), &mut ch.settings.minimum_delta_uv, &MIN_DELTA_UV, |v| {
                        format!("{:.0} uV", v)
                    });
                    combo_f64(ui, format!("lm{i}"), &mut ch.settings.lower_threshold_minimum, &LOWER_MIN_UV, |v| {
                        format!("{:.0} uV", v)
                    });
                    ui.end_row();
                }
            });
    });
}

fn combo_window(ui: &mut egui::Ui, i: usize, val: &mut EmgWindow) {
    egui::ComboBox::from_id_salt(format!("emg_w{i}"))
        .selected_text(val.label())
        .width(64.0)
        .show_ui(ui, |ui| {
            for w in EmgWindow::ALL {
                ui.selectable_value(val, w, w.label());
            }
        });
}

fn combo_limit(ui: &mut egui::Ui, i: usize, val: &mut EmgUvLimit) {
    egui::ComboBox::from_id_salt(format!("emg_l{i}"))
        .selected_text(val.label())
        .width(72.0)
        .show_ui(ui, |ui| {
            for w in EmgUvLimit::ALL {
                ui.selectable_value(val, w, w.label());
            }
        });
}

fn combo_creep(ui: &mut egui::Ui, id: String, val: &mut f64) {
    combo_f64(ui, id, val, &crate::emg::EmgCreep::ALL_INC, |v| format!("{}", v));
}

fn combo_f64(ui: &mut egui::Ui, id: String, val: &mut f64, options: &[f64], label: impl Fn(f64) -> String) {
    let text = label(*val);
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(72.0)
        .show_ui(ui, |ui| {
            for &o in options {
                ui.selectable_value(val, o, label(o));
            }
        });
}
