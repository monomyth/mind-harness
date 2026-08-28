//! OpenBCI GUI color palette and egui visuals.
//!
//! Values are taken from the Java/Processing reference
//! (`OpenBCI_GUI.pde` color constants) so the Rust port reads as the same app.

use eframe::egui::{self, Color32, Stroke, Visuals};

/// `OPENBCI_DARKBLUE` — primary text / chrome outline.
pub const OPENBCI_DARKBLUE: Color32 = Color32::from_rgb(1, 18, 41);
/// `OPENBCI_BLUE` — top navigation bar.
pub const OPENBCI_BLUE: Color32 = Color32::from_rgb(31, 69, 110);
/// `buttonsLightBlue` / sub-nav.
pub const SUBNAV_LIGHTBLUE: Color32 = Color32::from_rgb(57, 128, 204);
/// `TURN_ON_GREEN` — Start Data Stream.
pub const TURN_ON_GREEN: Color32 = Color32::from_rgb(195, 242, 181);
/// `TURN_OFF_RED` — Stop Data Stream.
pub const TURN_OFF_RED: Color32 = Color32::from_rgb(255, 210, 210);
/// `BOLD_RED`.
pub const BOLD_RED: Color32 = Color32::from_rgb(224, 56, 45);
pub const WHITE: Color32 = Color32::WHITE;
pub const GREY_235: Color32 = Color32::from_rgb(235, 235, 235);
#[allow(dead_code)]
pub const GREY_200: Color32 = Color32::from_rgb(200, 200, 200);
pub const OBJECT_BORDER_GREY: Color32 = Color32::from_rgb(150, 150, 150);
/// Widget header bar (Java `navHeight` chrome).
pub const WIDGET_HEADER: Color32 = Color32::from_rgb(150, 150, 150);
pub const WIDGET_BG: Color32 = Color32::from_rgb(250, 250, 255);
pub const ACCEL_X: Color32 = BOLD_RED;
pub const ACCEL_Y: Color32 = Color32::from_rgb(49, 113, 89);
pub const ACCEL_Z: Color32 = Color32::from_rgb(54, 87, 158);

/// Electrode-ribbon colors (Java `channelColors`, 8 entries, wrap for Daisy).
pub const CHANNEL_COLORS: [Color32; 8] = [
    Color32::from_rgb(129, 129, 129),
    Color32::from_rgb(124, 75, 141),
    Color32::from_rgb(54, 87, 158),
    Color32::from_rgb(49, 113, 89),
    Color32::from_rgb(221, 178, 13),
    Color32::from_rgb(253, 94, 52),
    Color32::from_rgb(224, 56, 45),
    Color32::from_rgb(162, 82, 49),
];

pub fn channel_color(index: usize) -> Color32 {
    CHANNEL_COLORS[index % CHANNEL_COLORS.len()]
}

pub fn apply_visuals(ctx: &egui::Context) {
    let mut visuals = Visuals::light();
    visuals.panel_fill = Color32::from_rgb(244, 246, 248);
    visuals.window_fill = WHITE;
    visuals.extreme_bg_color = GREY_235;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, OPENBCI_DARKBLUE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, OPENBCI_DARKBLUE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, OPENBCI_BLUE);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, OPENBCI_BLUE);
    visuals.selection.bg_fill = SUBNAV_LIGHTBLUE;
    visuals.selection.stroke = Stroke::new(1.0_f32, OPENBCI_BLUE);
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
    fn palette_matches_java_literals() {
        assert_eq!(OPENBCI_BLUE, Color32::from_rgb(31, 69, 110));
        assert_eq!(OPENBCI_DARKBLUE, Color32::from_rgb(1, 18, 41));
        assert_eq!(TURN_ON_GREEN, Color32::from_rgb(195, 242, 181));
    }
}
