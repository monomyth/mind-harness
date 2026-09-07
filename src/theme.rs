//! Studio chrome — ReBot Motion Lab quiet instrument (charcoal + lime).
//! Legacy Imagine SETUP_CYAN / SETUP_NEON kept as unused consts for one release.

use eframe::egui::{self, Color32, Stroke, Visuals};

/// Plot canvas / deepest stage.
pub const CANVAS: Color32 = Color32::from_rgb(0x11, 0x14, 0x13);
/// Properties rack / panels (legacy alias → PANEL_RAISED).
pub const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1e, 0x1b);
/// Transport strip and widget headers.
pub const TRANSPORT: Color32 = Color32::from_rgb(0x14, 0x18, 0x16);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xf0, 0xe8);
/// Hairline borders (quiet chrome).
pub const HAIRLINE: Color32 = Color32::from_rgb(0x34, 0x3c, 0x32);
/// Legacy muted amber (widgets still reference). Prefer ACCENT_LIME for chrome.
pub const ACCENT: Color32 = Color32::from_rgb(0xb0, 0x8d, 0x57);
/// Quiet start / stop fills.
pub const START: Color32 = Color32::from_rgb(0x3c, 0x47, 0x24);
pub const STOP: Color32 = Color32::from_rgb(0x6b, 0x3d, 0x3d);

// --- ReBot Motion Lab tokens (primary chrome) ---

/// High-chroma lime accent (~ReBot `--primary` #c3e85a, slightly lifted).
pub const ACCENT_LIME: Color32 = Color32::from_rgb(0xc3, 0xe8, 0x78);
/// Deepest charcoal (sidebar / app shell).
pub const PANEL_DEEP: Color32 = Color32::from_rgb(0x11, 0x14, 0x13);
/// Raised charcoal (cards / inspector / inputs).
pub const PANEL_RAISED: Color32 = Color32::from_rgb(0x1a, 0x1e, 0x1b);
/// Secondary / dim labels.
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xa1, 0xaa, 0xa0);
/// Muted metadata (footer, version).
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x82, 0x8e, 0x7b);
/// Quiet nav pill (selected sidebar row).
pub const NAV_PILL: Color32 = Color32::from_rgb(0x25, 0x2d, 0x22);
/// Soft selection wash under lime tick.
pub const SELECTION_WASH: Color32 = Color32::from_rgb(0x2a, 0x30, 0x2a);

/// Legacy Imagine cyan — kept unused; do not use for primary chrome.
pub const SETUP_CYAN: Color32 = Color32::from_rgb(0x2e, 0xe6, 0xd6);
/// Legacy neon Start — unused.
pub const SETUP_NEON: Color32 = Color32::from_rgb(0x00, 0xe8, 0x6a);
/// Quiet charcoal card on Session Setup (not frosted cyan glass).
pub const SETUP_GLASS: Color32 = Color32::from_rgba_premultiplied(0x1a, 0x1e, 0x1b, 0xe6);
/// Quiet edge for setup card (lime @ low alpha, not cyan).
pub const SETUP_GLASS_EDGE: Color32 = Color32::from_rgba_premultiplied(0x3c, 0x47, 0x24, 0x55);

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
    visuals.panel_fill = PANEL_DEEP;
    visuals.window_fill = PANEL_RAISED;
    visuals.extreme_bg_color = CANVAS;
    visuals.faint_bg_color = TRANSPORT;
    visuals.code_bg_color = CANVAS;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT_LIME;
    visuals.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.widgets.noninteractive.bg_fill = PANEL_RAISED;
    visuals.widgets.noninteractive.weak_bg_fill = TRANSPORT;
    visuals.widgets.noninteractive.bg_stroke = hairline();
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.bg_fill = TRANSPORT;
    visuals.widgets.inactive.weak_bg_fill = TRANSPORT;
    visuals.widgets.inactive.bg_stroke = hairline();
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_DIM);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x30, 0x2a);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x30, 0x2a);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_LIME);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.bg_fill = SELECTION_WASH;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_LIME);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.open.bg_fill = PANEL_RAISED;
    visuals.widgets.open.bg_stroke = hairline();
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.selection.bg_fill = SELECTION_WASH;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT_LIME);
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
        assert_eq!(CANVAS, Color32::from_rgb(0x11, 0x14, 0x13));
        assert_eq!(PANEL, Color32::from_rgb(0x1a, 0x1e, 0x1b));
        assert_eq!(TRANSPORT, Color32::from_rgb(0x14, 0x18, 0x16));
        assert_eq!(TEXT, Color32::from_rgb(0xec, 0xf0, 0xe8));
        assert_eq!(HAIRLINE, Color32::from_rgb(0x34, 0x3c, 0x32));
        assert_ne!(CANVAS, Color32::from_rgb(250, 250, 255));
        assert_ne!(TRANSPORT, Color32::from_rgb(31, 69, 110));
        assert_ne!(TEXT, Color32::from_rgb(1, 18, 41));
    }

    #[test]
    fn rebot_tokens_are_lime_quiet_charcoal() {
        assert_eq!(ACCENT_LIME, Color32::from_rgb(0xc3, 0xe8, 0x78));
        assert_eq!(PANEL_DEEP, Color32::from_rgb(0x11, 0x14, 0x13));
        assert_eq!(PANEL_RAISED, Color32::from_rgb(0x1a, 0x1e, 0x1b));
        assert_eq!(TEXT_DIM, Color32::from_rgb(0xa1, 0xaa, 0xa0));
        assert_ne!(ACCENT_LIME, SETUP_CYAN);
        assert_ne!(ACCENT_LIME, SETUP_NEON);
        assert_ne!(SETUP_GLASS, Color32::from_rgba_premultiplied(0x1e, 0x1e, 0x21, 0xb8));
    }

    #[test]
    fn legacy_imagine_tokens_kept_but_not_primary() {
        // Kept for one release; primary chrome must use ACCENT_LIME.
        assert_eq!(SETUP_CYAN, Color32::from_rgb(0x2e, 0xe6, 0xd6));
        assert_eq!(SETUP_NEON, Color32::from_rgb(0x00, 0xe8, 0x6a));
        assert_ne!(SETUP_NEON, START);
        assert_ne!(SETUP_CYAN, ACCENT_LIME);
    }
}
