//! Head Plot: 3D Ultracortex Mark IV mesh (egui/wgpu). Not a 2D oval.
//! Default 3/4 camera, slightly above; drag to orbit. Occupied holes: disc +
//! 10-20 name on the lattice hole. Empty holes: mesh opening only. Click a
//! hole to assign; chosen contact highlights that hole only. Contact lost =
//! wired hole goes hollow. Save writes the montage profile.

use crate::board::DataSource;
use crate::fft::band_powers_psd;
use crate::laterality::{latch_rails, occupied_band_fill, Rhythm, WINDOW_SEC};
use crate::theme;
use crate::widgets::mark_iv::{
    self, channel_at, default_map, hit_hole, is_hole, paint_frame, paint_frame_hiding_pair,
    paint_head, project_holes, Camera, DEFAULT_SITES, HEADSET_NAME,
};
use crate::widgets::Widget;
use eframe::egui;

#[derive(Clone, Debug)]
pub enum MontageUiAction {
    Select(String),
    Save,
    SaveAs(String),
}

pub const DEFAULT_PROFILE_NAME: &str = "8ch 10-20";
pub const SITE_R: f32 = 6.0;
const HIT_R: f32 = 14.0;
const CHOSEN_R: f32 = 9.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeadOverlayKind {
    #[default]
    Hemispheres,
    SlowWaves,
}

pub const LABELS: [&str; 8] = ["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"];

#[derive(Clone, Debug, Default)]
pub struct OverlaySite {
    pub idx: usize,
    pub fill: f32,
}

#[derive(Clone, Debug)]
pub struct HeadOverlayFrame {
    pub kind: HeadOverlayKind,
    pub caption: String,
    pub sites: Vec<OverlaySite>,
    pub railed: [bool; 8],
}

impl Default for HeadOverlayFrame {
    fn default() -> Self {
        Self {
            kind: HeadOverlayKind::Hemispheres,
            caption: String::new(),
            sites: Vec::new(),
            railed: [false; 8],
        }
    }
}

pub struct WHeadPlot {
    title: String,
    frame: HeadOverlayFrame,
    railed: [bool; 8],
    channel_psd: [[f64; 5]; 8],
    map: [String; 8],
    dirty: bool,
    profile_name: String,
    profile_names: Vec<String>,
    pending: Option<MontageUiAction>,
    assign_hole: Option<String>,
    save_as_open: bool,
    save_as_buf: String,
    orbit: Camera,
}

/// Recapture-only: OPENBCI_ASSIGN_HOLE=C3 (crop scripts cannot click).
/// Unset in normal sessions. Production click path already sets `assign_hole`.
fn recapture_assign_hole_from_name(name: &str) -> Option<String> {
    let idx = mark_iv::hole_index(name)?;
    mark_iv::hole_name(idx).map(str::to_string)
}

fn recapture_assign_hole_from_env() -> Option<String> {
    let raw = std::env::var("OPENBCI_ASSIGN_HOLE").ok()?;
    recapture_assign_hole_from_name(raw.trim())
}

impl WHeadPlot {
    pub fn new() -> Self {
        Self {
            title: "Head Plot".to_string(),
            frame: HeadOverlayFrame::default(),
            railed: [false; 8],
            channel_psd: [[0.0; 5]; 8],
            map: default_map(),
            dirty: false,
            profile_name: DEFAULT_PROFILE_NAME.to_string(),
            profile_names: vec![DEFAULT_PROFILE_NAME.to_string()],
            pending: None,
            assign_hole: recapture_assign_hole_from_env(),
            save_as_open: false,
            save_as_buf: String::new(),
            orbit: Camera::default(),
        }
    }

    pub fn set_frame(&mut self, frame: HeadOverlayFrame) {
        self.frame = frame;
    }

    /// Head Plot only: all occupied inserts, per-channel fill. Pair views
    /// (Left / right, Which first) use two holes. No pair caption, no band letter.
    pub fn overlay_frame(&self) -> HeadOverlayFrame {
        let band = Rhythm::Alpha.psd_index();
        let mut psd = [0.0_f64; 8];
        for i in 0..8 {
            if self.map[i].is_empty() || self.railed[i] {
                continue;
            }
            psd[i] = self.channel_psd[i][band];
        }
        let fill = occupied_band_fill(&psd);
        let sites = (0..8)
            .filter(|&i| !self.map[i].is_empty())
            .map(|idx| OverlaySite {
                idx,
                fill: fill[idx],
            })
            .collect();
        HeadOverlayFrame {
            kind: HeadOverlayKind::Hemispheres,
            caption: if self
                .railed
                .iter()
                .zip(self.map.iter())
                .any(|(dead, name)| *dead && !name.is_empty())
            {
                "Contact lost".into()
            } else {
                String::new()
            },
            sites,
            railed: self.railed,
        }
    }

    pub fn channel_map(&self) -> [String; 8] {
        self.map.clone()
    }

    pub fn channel_holes(&self) -> [String; 8] {
        self.map.clone()
    }

    pub fn channel_labels(&self) -> [String; 8] {
        self.map.clone()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_catalog(&mut self, names: Vec<String>, last: &str) {
        self.profile_names = names;
        if !self.dirty {
            self.profile_name = last.to_string();
        }
    }

    pub fn set_map_clean(&mut self, map: [String; 8], profile: &str) {
        self.map = map;
        self.profile_name = profile.to_string();
        self.dirty = false;
        self.assign_hole = None;
        // Recapture-only: montage wipe must not drop OPENBCI_ASSIGN_HOLE.
        self.apply_recapture_assign_hole();
    }

    pub fn set_profile_clean(&mut self, labels: [String; 8], holes: [String; 8], profile: &str) {
        let _ = labels;
        self.set_map_clean(holes, profile);
    }

    pub fn assign_channel(&mut self, ch: usize, hole: &str) {
        if ch >= 8 || !is_hole(hole) {
            return;
        }
        for slot in self.map.iter_mut() {
            if slot.eq_ignore_ascii_case(hole) {
                slot.clear();
            }
        }
        self.map[ch] = hole.to_string();
        self.dirty = true;
        self.assign_hole = None;
    }

    pub fn clear_hole(&mut self, hole: &str) {
        for slot in self.map.iter_mut() {
            if slot.eq_ignore_ascii_case(hole) {
                slot.clear();
                self.dirty = true;
            }
        }
        self.assign_hole = None;
    }

    pub fn take_action(&mut self) -> Option<MontageUiAction> {
        self.pending.take()
    }

    /// Recapture-only. Re-apply after montage wipe so a crop can pin the chosen hole.
    pub fn apply_recapture_assign_hole(&mut self) {
        if let Some(name) = recapture_assign_hole_from_env() {
            self.assign_hole = Some(name);
        }
    }

    fn occupied(&self, name: &str) -> Option<usize> {
        channel_at(&self.map, name)
    }
}

impl Default for WHeadPlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Left / right and Which first pane: same Mark IV as Head Plot, only `sites`
/// named/filled (the caption pair — two gold indices). No assign, no path.
pub fn paint_pair_head(
    ui: &mut egui::Ui,
    orbit: &mut Camera,
    caption: &str,
    sites: &[OverlaySite],
    railed: &[bool; 8],
) {
    if !caption.is_empty() {
        ui.label(caption);
    }
    ui.add_space(2.0);
    let avail = ui.available_size();
    let desired = egui::vec2(avail.x.max(40.0), (avail.y - 8.0).max(40.0));
    let (resp, painter) = ui.allocate_painter(desired, egui::Sense::drag());
    let rect = resp.rect;
    if resp.dragged() {
        orbit.drag(resp.drag_delta());
    }
    painter.rect_filled(rect, 0.0, theme::CANVAS);
    if mark_iv::mesh().faces.is_empty() {
        painter.text(
            egui::pos2(rect.center().x, rect.center().y),
            egui::Align2::CENTER_CENTER,
            "Mark IV mesh failed to load — 2D outline fallback",
            egui::FontId::proportional(13.0),
            theme::STOP,
        );
    } else {
        paint_head(&painter, rect, *orbit);
        let pair: Vec<&str> = sites
            .iter()
            .filter(|s| s.idx < 8)
            .map(|s| DEFAULT_SITES[s.idx])
            .collect();
        if pair.len() >= 2 {
            paint_frame_hiding_pair(&painter, rect, *orbit, pair[0], pair[1]);
        } else {
            paint_frame(&painter, rect, *orbit);
        }
    }

    let map = default_map();
    let mut fill_at = [0.0_f32; 8];
    let mut show = [false; 8];
    for s in sites {
        if s.idx < 8 {
            fill_at[s.idx] = s.fill.clamp(0.0, 1.0);
            show[s.idx] = true;
        }
    }
    let projected = project_holes(rect, *orbit);
    let mut draw_order: Vec<usize> = (0..projected.len()).collect();
    draw_order.sort_by(|&a, &b| {
        projected[a]
            .depth
            .partial_cmp(&projected[b].depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for idx in draw_order {
        let pr = projected[idx];
        if !rect.expand(8.0).contains(pr.pos) {
            continue;
        }
        let name = match mark_iv::hole_name(pr.index) {
            Some(n) => n,
            None => continue,
        };
        let Some(ch) = channel_at(&map, name) else {
            continue;
        };
        if !show[ch] {
            continue;
        }
        if railed[ch] {
            painter.circle_stroke(pr.pos, SITE_R, egui::Stroke::new(1.25_f32, theme::STOP));
        } else {
            let t = fill_at[ch];
            let fill = if t > 0.02 {
                let a = (40.0 + t * 200.0) as u8;
                egui::Color32::from_rgba_unmultiplied(0xb0, 0x8d, 0x57, a)
            } else {
                theme::HAIRLINE
            };
            painter.circle_filled(pr.pos, SITE_R, fill);
            painter.circle_stroke(pr.pos, SITE_R, theme::hairline());
        }
        painter.text(
            pr.pos,
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(11.0),
            theme::TEXT,
        );
    }
}

pub(crate) fn head_canvas_radius(width: f32, height: f32) -> f32 {
    (width.min(height) * 0.40).max(40.0)
}

impl Widget for WHeadPlot {
    fn title(&self) -> &str {
        &self.title
    }

    fn update(&mut self, source: &dyn DataSource) {
        let sr = source.sample_rate() as f64;
        if sr <= 1.0 {
            return;
        }
        let n = (WINDOW_SEC * sr).round() as usize;
        let raw_rows = source.get_raw_data(n.max(32));
        let exg = source.exg_channels();
        let mut chs = Vec::new();
        for &col in exg.iter().take(8) {
            chs.push(
                raw_rows
                    .iter()
                    .map(|row| row.get(col).copied().unwrap_or(0.0))
                    .collect::<Vec<f64>>(),
            );
        }
        latch_rails(&mut self.railed, &chs);
        self.channel_psd = [[0.0; 5]; 8];
        let data = source.get_data(n.max(32));
        if exg.len() < 8 || data.is_empty() {
            return;
        }
        for (ch, &col) in exg.iter().take(8).enumerate() {
            if self.railed[ch] {
                continue;
            }
            let samples: Vec<f64> = data
                .iter()
                .map(|row| row.get(col).copied().unwrap_or(0.0))
                .collect();
            if samples.is_empty() {
                continue;
            }
            self.channel_psd[ch] = band_powers_psd(&samples, sr);
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        _source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.horizontal(|ui| {
            // Own overlay caption only (empty or Contact lost). Never a pair/band line
            // injected via set_frame — that is how P3/P4 alpha reappeared on this pane.
            let plate_caption = self.overlay_frame().caption;
            if !plate_caption.is_empty() {
                ui.label(&plate_caption);
                ui.add_space(8.0);
            }
            let mut headset = HEADSET_NAME.to_string();
            egui::ComboBox::from_id_salt("head_headset")
                .selected_text(HEADSET_NAME)
                .width(168.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut headset, HEADSET_NAME.to_string(), HEADSET_NAME);
                });
            let _ = headset;
            let shown = if self.dirty {
                format!("{}*", self.profile_name)
            } else {
                self.profile_name.clone()
            };
            let mut pick = self.profile_name.clone();
            let names = self.profile_names.clone();
            egui::ComboBox::from_id_salt("head_montage_profile")
                .selected_text(shown)
                .width(110.0)
                .show_ui(ui, |ui| {
                    for n in &names {
                        ui.selectable_value(&mut pick, n.clone(), n);
                    }
                });
            if pick != self.profile_name {
                self.pending = Some(MontageUiAction::Select(pick));
            }
            let save = egui::Button::new("Save").small().frame(false);
            if ui
                .add(save)
                .on_hover_text("Write channel → 10-20 hole into the active profile")
                .clicked()
            {
                self.pending = Some(MontageUiAction::Save);
            }
            if ui
                .add(egui::Button::new("Save as").small().frame(false))
                .clicked()
            {
                self.save_as_open = true;
            }
        });
        if self.save_as_open {
            ui.horizontal(|ui| {
                ui.label("Name");
                let resp =
                    ui.add(egui::TextEdit::singleline(&mut self.save_as_buf).desired_width(140.0));
                if ui.button("Create").clicked()
                    || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    let name = self.save_as_buf.trim().to_string();
                    if !name.is_empty() {
                        self.pending = Some(MontageUiAction::SaveAs(name));
                        self.save_as_open = false;
                        self.save_as_buf.clear();
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.save_as_open = false;
                }
            });
        }
        if let Some(hole) = self.assign_hole.clone() {
            ui.horizontal(|ui| {
                ui.label(format!("Wire {hole}"));
                for ch in 0..8 {
                    let cur = if self.map[ch].is_empty() {
                        "—".to_string()
                    } else {
                        self.map[ch].clone()
                    };
                    let txt = format!("{} {cur}", ch + 1);
                    if ui.add(egui::Button::new(txt).small()).clicked() {
                        self.assign_channel(ch, &hole);
                    }
                }
                if ui.add(egui::Button::new("Unassign").small()).clicked() {
                    self.clear_hole(&hole);
                }
                if ui
                    .add(egui::Button::new("Cancel").small().frame(false))
                    .clicked()
                {
                    self.assign_hole = None;
                }
            });
        }
        ui.add_space(2.0);

        let avail = ui.available_size();
        let desired = egui::vec2(avail.x.max(40.0), (avail.y - 8.0).max(40.0));
        let (resp, painter) = ui.allocate_painter(desired, egui::Sense::click_and_drag());
        let rect = resp.rect;

        if resp.dragged() {
            self.orbit.drag(resp.drag_delta());
        }
        let projected = project_holes(rect, self.orbit);
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some(i) = hit_hole(pos, &projected, HIT_R) {
                    if let Some(name) = mark_iv::hole_name(i) {
                        self.assign_hole = Some(name.to_string());
                    }
                }
            }
        }

        painter.rect_filled(rect, 0.0, theme::CANVAS);
        if mark_iv::mesh().faces.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "Mark IV mesh failed to load — 2D outline fallback",
                egui::FontId::proportional(13.0),
                theme::STOP,
            );
        } else {
            paint_head(&painter, rect, self.orbit);
            paint_frame(&painter, rect, self.orbit);
        }

        let plate = self.overlay_frame();
        let mut fill_at = [0.0_f32; 8];
        for s in &plate.sites {
            if s.idx < 8 {
                fill_at[s.idx] = s.fill.clamp(0.0, 1.0);
            }
        }

        let mut draw_order: Vec<usize> = (0..projected.len()).collect();
        draw_order.sort_by(|&a, &b| {
            projected[a]
                .depth
                .partial_cmp(&projected[b].depth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let scale = mark_iv::canvas_scale(rect);
        for idx in draw_order {
            let pr = projected[idx];
            if !rect.expand(8.0).contains(pr.pos) {
                continue;
            }
            let name = match mark_iv::hole_name(pr.index) {
                Some(n) => n,
                None => continue,
            };
            let chosen = self
                .assign_hole
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(name));
            let occ = self.occupied(name);
            if occ.is_none() && !chosen {
                // Unoccupied: mesh opening is empty. No badge.
                continue;
            }
            // Insert rim is ~0.12 mesh units; 6px is a spec in a 90px socket.
            // Disc sits inside the opening. Centers stay the 35 named inserts.
            let persp = 3.4 / (3.4 - pr.depth).max(0.35);
            let disc_r = (0.12 * scale * persp * 0.48).max(SITE_R);
            if chosen {
                // Chosen contact: highlight this hole only — filled disc, not a
                // ring, not every live site. Clears with assign_hole.
                painter.circle_filled(
                    pr.pos,
                    disc_r.max(CHOSEN_R),
                    egui::Color32::from_rgba_unmultiplied(0xe8, 0xc0, 0x7a, 220),
                );
            }
            if let Some(ch) = occ {
                let railed = self.frame.railed[ch] || self.railed[ch];
                if railed {
                    painter.circle_stroke(pr.pos, disc_r, egui::Stroke::new(1.25_f32, theme::STOP));
                    painter.text(
                        pr.pos + egui::vec2(0.0, disc_r + 2.0),
                        egui::Align2::CENTER_TOP,
                        "Contact lost",
                        egui::FontId::proportional(10.0),
                        theme::STOP,
                    );
                } else {
                    let t = fill_at[ch];
                    let fill = if t > 0.02 {
                        let a = (120.0 + t * 135.0) as u8;
                        egui::Color32::from_rgba_unmultiplied(0xb0, 0x8d, 0x57, a)
                    } else {
                        theme::HAIRLINE
                    };
                    painter.circle_filled(pr.pos, disc_r, fill);
                    painter.circle_stroke(pr.pos, disc_r, theme::hairline());
                    painter.text(
                        pr.pos,
                        egui::Align2::CENTER_CENTER,
                        name,
                        egui::FontId::proportional(11.0),
                        theme::TEXT,
                    );
                }
            } else {
                painter.text(
                    pr.pos,
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(11.0),
                    theme::TEXT,
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::mark_iv;

    #[test]
    fn radius_fills_large_pane_without_85_cap() {
        let r = head_canvas_radius(400.0, 400.0);
        assert!((r - 160.0).abs() < 1e-3, "got {r}");
        assert!(r > 85.0);
    }

    #[test]
    fn radius_floor_for_rack_card() {
        assert_eq!(head_canvas_radius(50.0, 50.0), 40.0);
    }

    #[test]
    fn sites_are_one_size() {
        assert_eq!(SITE_R, 6.0);
    }

    #[test]
    fn default_caption_has_no_pair_and_no_alpha() {
        let c = HeadOverlayFrame::default().caption;
        assert!(!c.contains("P3/P4"), "{c}");
        assert!(!c.contains('α'), "{c}");
        assert!(!c.contains("8–13"), "{c}");
        assert!(!c.to_lowercase().contains("left brain"), "{c}");
        assert!(!c.to_lowercase().contains("more active"), "{c}");
    }

    #[test]
    fn default_profile_is_mark_iv_cyton_8ch() {
        let w = WHeadPlot::new();
        assert_eq!(w.channel_map(), LABELS.map(|s| s.to_string()));
        assert_eq!(
            LABELS,
            ["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"]
        );
        assert_eq!(w.occupied("O1"), Some(6));
        assert_eq!(w.occupied("O2"), Some(7));
        assert_eq!(w.occupied("P7"), Some(4));
        assert_eq!(w.occupied("P8"), Some(5));
        assert_eq!(w.occupied("C3"), Some(2));
        assert_eq!(w.occupied("C4"), Some(3));
        assert_eq!(w.occupied("F7"), None);
        assert_eq!(w.occupied("P3"), None);
        assert_eq!(w.occupied("P4"), None);
        assert_eq!(HEADSET_NAME, "Ultracortex Mark IV");
        assert!(mark_iv::mesh().holes.iter().any(|h| h.name == "O1"));
        assert!(mark_iv::mesh().holes.iter().any(|h| h.name == "P3"));
    }

    #[test]
    fn occupied_vs_empty_after_assign_and_unassign() {
        let mut w = WHeadPlot::new();
        assert!(w.occupied("P3").is_none());
        assert_eq!(w.occupied("O2"), Some(7));
        w.assign_channel(7, "P3");
        assert!(w.is_dirty());
        assert_eq!(w.occupied("P3"), Some(7));
        assert!(w.occupied("O2").is_none());
        assert_eq!(w.channel_map()[7], "P3");
        w.clear_hole("P3");
        assert!(w.occupied("P3").is_none());
        assert!(w.channel_map()[7].is_empty());
    }

    #[test]
    fn assign_clears_previous_occupant() {
        let mut w = WHeadPlot::new();
        w.assign_channel(0, "F7");
        assert_eq!(w.channel_map()[0], "F7");
        assert_eq!(w.channel_map()[2], "C3");
        assert_eq!(w.occupied("F7"), Some(0));
        assert_eq!(w.occupied("Fp1"), None);
    }

    #[test]
    fn persist_round_trip_via_set_map_clean() {
        let mut w = WHeadPlot::new();
        w.assign_channel(2, "T7");
        let holes = w.channel_map();
        let mut w2 = WHeadPlot::new();
        w2.set_map_clean(holes.clone(), "Eugene cap");
        assert_eq!(w2.channel_map(), holes);
        assert!(!w2.is_dirty());
        assert_eq!(w2.occupied("T7"), Some(2));
        assert!(w2.occupied("C3").is_none());
    }

    #[test]
    fn recapture_assign_hole_env_name_is_that_hole_only() {
        assert_eq!(recapture_assign_hole_from_name("C3").as_deref(), Some("C3"));
        assert_eq!(recapture_assign_hole_from_name("c3").as_deref(), Some("C3"));
        assert!(recapture_assign_hole_from_name("").is_none());
        assert!(recapture_assign_hole_from_name("not-a-site").is_none());
        let mut w = WHeadPlot::new();
        w.assign_hole = recapture_assign_hole_from_name("C3");
        assert_eq!(w.assign_hole.as_deref(), Some("C3"));
        assert!(
            w.occupied("C3").is_some(),
            "C3 is occupied, not a rest overlay"
        );
        assert_ne!(w.assign_hole.as_deref(), Some("P3"));
        assert_ne!(w.assign_hole.as_deref(), Some("P4"));
        w.set_map_clean(default_map(), DEFAULT_PROFILE_NAME);
        assert!(
            w.assign_hole.is_none(),
            "set_map_clean still clears when recapture env is unset"
        );
    }

    #[test]
    fn chosen_contact_is_that_assign_hole_only_and_clears() {
        let mut w = WHeadPlot::new();
        assert!(w.assign_hole.is_none());
        w.assign_hole = Some("O1".to_string());
        assert_eq!(w.assign_hole.as_deref(), Some("O1"));
        assert!(w.occupied("Fp1").is_some());
        assert_ne!(w.assign_hole.as_deref(), w.occupied("Fp1").map(|_| "Fp1"));
        w.assign_channel(0, "O1");
        assert!(
            w.assign_hole.is_none(),
            "assign completes → clear highlight"
        );
        w.assign_hole = Some("Cz".to_string());
        w.clear_hole("Cz");
        assert!(
            w.assign_hole.is_none(),
            "unassign completes → clear highlight"
        );
        w.assign_hole = Some("P7".to_string());
        w.set_map_clean(default_map(), DEFAULT_PROFILE_NAME);
        assert!(w.assign_hole.is_none(), "set_map_clean clears chosen hole");
    }

    #[test]
    fn empty_inserts_draw_nothing() {
        let src = include_str!("head_plot.rs");
        assert!(src.contains("if occ.is_none() && !chosen"));
        assert!(src.contains("Unoccupied: mesh opening is empty"));
        assert!(src.contains("continue;"));
        assert!(src.contains("fill_at[ch]"));
    }

    #[test]
    fn occupied_fill_is_per_channel_not_only_p3_p4() {
        use crate::laterality::occupied_band_fill;
        let band = [0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.8, 0.8];
        let fill = occupied_band_fill(&band);
        assert_eq!(fill.len(), 8);
        assert!(fill.iter().filter(|&&f| f > 0.02).count() >= 8);
        assert!(fill[0] > 0.02, "Fp1 is live, not pair-only");
        assert!(
            fill[6] > fill[0],
            "O1 louder than Fp1 from its own band power"
        );
    }

    #[test]
    fn empty_insert_p3_is_not_occupied_and_gets_no_name() {
        assert!(mark_iv::DEFAULT_SITES.contains(&"O1"));
        assert!(mark_iv::DEFAULT_SITES.contains(&"O2"));
        assert!(!mark_iv::DEFAULT_SITES.contains(&"P3"));
        assert!(!mark_iv::DEFAULT_SITES.contains(&"F7"));
        let w = WHeadPlot::new();
        assert_eq!(w.occupied("P3"), None);
        assert_eq!(w.occupied("P4"), None);
        assert_eq!(w.occupied("F7"), None);
        assert!(!w.channel_map().iter().any(|n| n.eq_ignore_ascii_case("P3")));
    }

    #[test]
    fn overlay_frame_fills_eight_occupied() {
        let mut w = WHeadPlot::new();
        let i = Rhythm::Alpha.psd_index();
        for ch in 0..8 {
            w.channel_psd[ch][i] = 1.0 + ch as f64 * 0.1;
        }
        let frame = w.overlay_frame();
        assert_eq!(frame.sites.len(), 8);
        for s in &frame.sites {
            assert!(s.fill > 0.3, "ch {} fill {}", s.idx, s.fill);
        }
        assert!(!frame.caption.contains('α'), "{}", frame.caption);
        assert!(!frame.caption.contains("P3/P4"), "{}", frame.caption);
        w.railed[2] = true;
        let frame = w.overlay_frame();
        assert_eq!(frame.sites.iter().find(|s| s.idx == 2).unwrap().fill, 0.0);
        assert_eq!(frame.sites.len(), 8);
        assert_eq!(frame.caption, "Contact lost");
        assert!(!frame.caption.contains('α'));
        assert!(!frame.caption.contains("P3/P4"));
    }

    #[test]
    fn head_plot_is_mark_iv_3d_with_orbit_and_empty_holes() {
        let src = include_str!("head_plot.rs");
        assert!(src.contains("Ultracortex Mark IV"));
        assert!(src.contains("paint_head"));
        assert!(src.contains("paint_frame"));
        assert!(src.contains("Unassign"));
        assert!(src.contains("Save as"));
        assert!(src.contains("head_headset"));
        assert!(src.contains(concat!("orbit.", "drag")));
        assert!(src.contains(concat!("click_and_", "drag")));
        assert!(!src.contains(concat!("EMPTY_", "R")));
        assert!(src.contains("CHOSEN_R"));
        assert!(src.contains("assign_hole"));
        assert!(src.contains("OPENBCI_ASSIGN_HOLE"));
        assert!(src.contains("recapture_assign_hole_from_env"));
        assert!(src.contains("CENTER_CENTER"));
        assert!(
            src.contains("\"Contact lost\""),
            "dead hole must say Contact lost"
        );
        assert!(!src.contains(concat!("head_", "outline")));
        assert!(!src.contains(concat!("ELLIPSE_", "RX")));
        assert!(!src.contains(concat!("SITE_", "XY")));
        assert!(!src.contains(concat!("version ", "IV")));
    }
}
