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

/// Session Setup Imagine — cyan selection rim (not amber studio accent).
pub const SETUP_CYAN: Color32 = Color32::from_rgb(0x2e, 0xe6, 0xd6);
/// Neon Start for Session Setup (Cinema Imagine). Not the quiet studio START.
pub const SETUP_NEON: Color32 = Color32::from_rgb(0x00, 0xe8, 0x6a);
/// Frosted glass card on the Data Source + Serial stack.
pub const SETUP_GLASS: Color32 = Color32::from_rgba_premultiplied(0x1e, 0x1e, 0x21, 0xb8);
/// Cyan rim on the glass card (premul SETUP_CYAN @ ~0x55).
pub const SETUP_GLASS_EDGE: Color32 = Color32::from_rgba_premultiplied(0x0f, 0x4c, 0x47, 0x55);

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

/// Occupied-insert activity fill: dark → channel hue → hot, opaque.
pub fn activity_fill_color(channel: usize, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let dark = Color32::from_rgb(0x2a, 0x2a, 0x2a);
    let mid = channel_color(channel);
    let hot = Color32::from_rgb(0xf2, 0xd4, 0x8a);
    if t <= 0.5 {
        lerp_rgb(dark, mid, t * 2.0)
    } else {
        lerp_rgb(mid, hot, (t - 0.5) * 2.0)
    }
}

fn lerp_rgb(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}

/// UI type sizes (Settings → Fonts). Bigger defaults so Experiments marks read.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct FontSizes {
    pub small: f32,
    pub body: f32,
    pub button: f32,
    pub heading: f32,
    pub mono: f32,
    pub marks: f32,
    pub hole_label: f32,
    pub caption: f32,
}

impl Default for FontSizes {
    fn default() -> Self {
        Self {
            small: 13.0,
            body: 16.0,
            button: 16.0,
            heading: 20.0,
            mono: 14.0,
            marks: 15.0,
            hole_label: 13.0,
            caption: 16.0,
        }
    }
}

impl FontSizes {
    pub fn apply_egui(&self, ctx: &egui::Context) {
        use egui::{FontFamily, FontId, TextStyle};
        let mut style = (*ctx.style()).clone();
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(self.small, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Body,
            FontId::new(self.body, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(self.button, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(self.heading, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(self.mono, FontFamily::Monospace),
        );
        ctx.set_style(style);
    }
}

static FONT_SIZES: std::sync::Mutex<FontSizes> = std::sync::Mutex::new(FontSizes {
    small: 13.0,
    body: 16.0,
    button: 16.0,
    heading: 20.0,
    mono: 14.0,
    marks: 15.0,
    hole_label: 13.0,
    caption: 16.0,
});

pub fn font_sizes() -> FontSizes {
    FONT_SIZES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn set_font_sizes(sizes: FontSizes) {
    if let Ok(mut g) = FONT_SIZES.lock() {
        *g = sizes;
    }
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
    fn activity_fill_changes_color_with_activity_not_gold_alpha_only() {
        let quiet = activity_fill_color(0, 0.1);
        let mid = activity_fill_color(0, 0.5);
        let hot = activity_fill_color(0, 1.0);
        assert_ne!(quiet, mid);
        assert_ne!(mid, hot);
        assert_eq!(mid, CHANNEL_COLORS[0]);
        assert_ne!(activity_fill_color(0, 0.5), activity_fill_color(2, 0.5));
        assert_ne!(hot, Color32::from_rgba_unmultiplied(0xb0, 0x8d, 0x57, 200));
        assert_eq!(hot.a(), 255);
        assert_eq!(mid.a(), 255);
    }

    #[test]
    fn font_size_defaults_are_big_enough_for_marks_log() {
        let f = FontSizes::default();
        assert_eq!(f.small, 13.0);
        assert_eq!(f.body, 16.0);
        assert_eq!(f.button, 16.0);
        assert_eq!(f.heading, 20.0);
        assert_eq!(f.mono, 14.0);
        assert_eq!(f.marks, 15.0);
        assert_eq!(f.hole_label, 13.0);
        assert_eq!(f.caption, 16.0);
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

    #[test]
    fn session_setup_imagine_tokens_are_neon_not_quiet() {
        assert_eq!(SETUP_CYAN, Color32::from_rgb(0x2e, 0xe6, 0xd6));
        assert_eq!(SETUP_NEON, Color32::from_rgb(0x00, 0xe8, 0x6a));
        assert_ne!(SETUP_NEON, START);
        assert_ne!(SETUP_NEON, TURN_ON_GREEN);
        assert_ne!(SETUP_CYAN, ACCENT);
    }
}
