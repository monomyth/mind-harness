//! W_EMGJoystick — 2D EMG joystick (Java `W_EMGJoystick`).
//!
//! X = ch(+) − ch(−), Y = ch(up) − ch(down), mapped to a unit circle and lerped.

use crate::board::DataSource;
use crate::emg::map_to_unit_circle;
use crate::theme;
use crate::widgets::emg::{emg_settings_ui, paint_emg_cell};
use crate::widgets::Widget;
use eframe::egui;
use egui::{pos2, Color32, Rect, Stroke};

const SMOOTH: &[(&str, f64)] = &[
    ("Off", 0.0),
    ("0.9", 0.9),
    ("0.95", 0.95),
    ("0.98", 0.98),
    ("0.99", 0.99),
    ("0.999", 0.999),
    ("0.9999", 0.9999),
];

pub struct WEmgJoystick {
    title: String,
    /// Java `emgJoystickInputs`: −X, +X, +Y, −Y channel indices.
    inputs: [usize; 4],
    smoothing: f64,
    x: f64,
    y: f64,
    show_settings: bool,
}

impl WEmgJoystick {
    pub fn new() -> Self {
        Self {
            title: "EMG Joystick".to_string(),
            inputs: [0, 1, 2, 3],
            smoothing: 0.9,
            x: 0.0,
            y: 0.0,
            show_settings: false,
        }
    }

    fn step(&mut self, emg: &crate::emg::EmgProcessor) {
        let n = emg.channels.len();
        if n == 0 {
            self.x = 0.0;
            self.y = 0.0;
            return;
        }
        let v = |i: usize| {
            let ch = self.inputs[i].min(n - 1);
            emg.channels[ch].output_normalized
        };
        let raw_x = v(1) - v(0);
        let raw_y = v(2) - v(3);
        let (nx, ny) = map_to_unit_circle(raw_x, raw_y);
        let amount = 1.0 - self.smoothing.clamp(0.0, 0.9999);
        self.x += (nx - self.x) * amount;
        self.y += (ny - self.y) * amount;
    }
}

impl Default for WEmgJoystick {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WEmgJoystick {
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
        let n = source
            .exg_channels()
            .len()
            .min(ctx.emg.channels.len())
            .max(1);
        for i in 0..4 {
            self.inputs[i] = self.inputs[i].min(n - 1);
        }
        self.step(ctx.emg);

        ui.horizontal(|ui| {
            if ui.button("EMG Settings").clicked() {
                self.show_settings = !self.show_settings;
            }
            ui.label("Smooth");
            let cur = SMOOTH
                .iter()
                .find(|(_, v)| (*v - self.smoothing).abs() < 1e-9)
                .map(|(s, _)| *s)
                .unwrap_or("0.9");
            egui::ComboBox::from_id_salt("emg_joy_smooth")
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    for &(lab, v) in SMOOTH {
                        if ui
                            .selectable_label((self.smoothing - v).abs() < 1e-9, lab)
                            .clicked()
                        {
                            self.smoothing = v;
                        }
                    }
                });
        });

        if self.show_settings {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                emg_settings_ui(ui, ctx.emg);
            });
        }

        ui.horizontal(|ui| {
            axis_picker(ui, "−X", n, &mut self.inputs[0]);
            axis_picker(ui, "+X", n, &mut self.inputs[1]);
            axis_picker(ui, "+Y", n, &mut self.inputs[2]);
            axis_picker(ui, "−Y", n, &mut self.inputs[3]);
            ui.small(format!("x={:.2}  y={:.2}", self.x, self.y));
        });

        let avail = ui.available_size();
        let side = avail.x.min(avail.y).max(80.0);
        let (plot_rect, _) =
            ui.allocate_exact_size(egui::vec2(avail.x, side.max(120.0)), egui::Sense::hover());
        paint_joystick(ui, plot_rect, self.x, self.y);

        let mini_h = 56.0_f32;
        ui.horizontal(|ui| {
            let labels = ["−X", "+X", "+Y", "−Y"];
            for (i, lab) in labels.iter().enumerate() {
                let ch = self.inputs[i];
                ui.vertical(|ui| {
                    ui.small(format!("{lab} ch{}", ch + 1));
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(88.0, mini_h), egui::Sense::hover());
                    if let Some(state) = ctx.emg.channels.get(ch) {
                        paint_emg_cell(ui, r, ch, state);
                    }
                });
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

fn axis_picker(ui: &mut egui::Ui, label: &str, n: usize, ch: &mut usize) {
    ui.label(label);
    egui::ComboBox::from_id_salt(format!("emg_joy_{label}"))
        .selected_text(format!("{}", *ch + 1))
        .width(40.0)
        .show_ui(ui, |ui| {
            for i in 0..n {
                ui.selectable_value(ch, i, format!("{}", i + 1));
            }
        });
}

fn paint_joystick(ui: &egui::Ui, rect: Rect, x: f64, y: f64) {
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let d = rect.width().min(rect.height()) * 0.72;
    let r = d * 0.5;
    painter.circle_filled(c, r, Color32::from_rgb(245, 245, 245));
    painter.circle_stroke(c, r, Stroke::new(1.0_f32, Color32::from_rgb(210, 210, 210)));
    painter.line_segment(
        [pos2(c.x - r, c.y), pos2(c.x + r, c.y)],
        Stroke::new(1.0_f32, Color32::from_gray(180)),
    );
    painter.line_segment(
        [pos2(c.x, c.y - r), pos2(c.x, c.y + r)],
        Stroke::new(1.0_f32, Color32::from_gray(180)),
    );
    let inset = 15.0 * 2.0;
    let span = (r - inset).max(8.0);
    let px = c.x + (x.clamp(-1.0, 1.0) as f32) * span;
    // Java maps joystick Y with inverted draw (up is +Y).
    let py = c.y - (y.clamp(-1.0, 1.0) as f32) * span;
    let p = pos2(px, py);
    painter.circle_stroke(p, 7.5, Stroke::new(2.0_f32, theme::OPENBCI_BLUE));
    painter.line_segment(
        [pos2(p.x - 10.0, p.y), pos2(p.x + 10.0, p.y)],
        Stroke::new(2.0_f32, theme::OPENBCI_BLUE),
    );
    painter.line_segment(
        [pos2(p.x, p.y - 10.0), pos2(p.x, p.y + 10.0)],
        Stroke::new(2.0_f32, theme::OPENBCI_BLUE),
    );
}
