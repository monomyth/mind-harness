//! Studio chrome — Ableton / Blender / Resolve greys, not Java OpenBCI navy.

use eframe::egui::{self, Color32, Stroke, Visuals};

/// Plot canvas.
pub const CANVAS: Color32 = Color32::from_rgb(0x1d, 0x1d, 0x1d);
/// Properties rack / panels.
pub const PANEL: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
/// Transport strip and widget headers.
pub const TRANSPORT: Color32 = Color32::from_rgb(0x23, 0x23, 0x23);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
/// Hairline borders.
pub const HAIRLINE: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d);
/// Muted amber accent (not Java light-blue tiles).
pub const ACCENT: Color32 = Color32::from_rgb(0xb0, 0x8d, 0x57);
/// Quiet start / stop.
pub const START: Color32 = Color32::from_rgb(0x3d, 0x6b, 0x45);
pub const STOP: Color32 = Color32::from_rgb(0x6b, 0x3d, 0x3d);

pub const TURN_ON_GREEN: Color32 = START;
pub const BOLD_RED: Color32 = Color32::from_rgb(0xc4, 0x5c, 0x4e);
pub const ACCEL_X: Color32 = BOLD_RED;
pub const ACCEL_Y: Color32 = Color32::from_rgb(0x6a, 0xa3, 0x7a);
pub const ACCEL_Z: Color32 = Color32::from_rgb(0x6a, 0x8c, 0xb4);

/// Electrode-ribbon hues (same order as Java `channelColors`, lifted for a dark canvas).
pub const CHANNEL_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x9a, 0x9a, 0x9a),
    Color32::from_rgb(0x9a, 0x6a, 0xb0),
    Color32::from_rgb(0x5a, 0x7a, 0xc4),
    Color32::from_rgb(0x4a, 0x9a, 0x6a),
    Color32::from_rgb(0xd4, 0xb4, 0x3a),
    Color32::from_rgb(0xe0, 0x7a, 0x48),
    Color32::from_rgb(0xd0, 0x50, 0x44),
    Color32::from_rgb(0xb0, 0x70, 0x48),
];

pub fn channel_color(index: usize) -> Color32 {
    CHANNEL_COLORS[index % CHANNEL_COLORS.len()]
}

pub fn hairline() -> Stroke {
    Stroke::new(1.0_f32, HAIRLINE)
}

pub fn apply_visuals(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = CANVAS;
    visuals.faint_bg_color = TRANSPORT;
    visuals.code_bg_color = CANVAS;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.window_stroke = hairline();
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = TRANSPORT;
    visuals.widgets.noninteractive.bg_stroke = hairline();
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.bg_fill = TRANSPORT;
    visuals.widgets.inactive.weak_bg_fill = TRANSPORT;
    visuals.widgets.inactive.bg_stroke = hairline();
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x40, 0x3a, 0x30);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.open.bg_fill = PANEL;
    visuals.widgets.open.bg_stroke = hairline();
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.selection.bg_fill = Color32::from_rgb(0x4a, 0x3e, 0x2a);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    ctx.set_visuals(visuals);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_colors_wrap_like_java() {
        assert_eq!(channel_color(0), CHANNEL_COLORS[0]);
        assert_eq!(channel_color(8), CHANNEL_COLORS[0]);
        assert_eq!(channel_color(15), CHANNEL_COLORS[7]);
    }

    #[test]
    fn palette_is_dark_studio_not_java_navy() {
        assert_eq!(CANVAS, Color32::from_rgb(0x1d, 0x1d, 0x1d));
        assert_eq!(PANEL, Color32::from_rgb(0x2b, 0x2b, 0x2b));
        assert_eq!(TRANSPORT, Color32::from_rgb(0x23, 0x23, 0x23));
        assert_eq!(TEXT, Color32::from_rgb(0xe6, 0xe6, 0xe6));
        assert_eq!(HAIRLINE, Color32::from_rgb(0x3d, 0x3d, 0x3d));
        assert_ne!(CANVAS, Color32::from_rgb(250, 250, 255));
        assert_ne!(TRANSPORT, Color32::from_rgb(31, 69, 110));
        assert_ne!(TEXT, Color32::from_rgb(1, 18, 41));
    }
}
