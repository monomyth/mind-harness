//! Left / right pane: Mark IV, caption pair's two holes. Not a hemisphere state.

use crate::board::DataSource;
use crate::fft::band_powers_psd;
use crate::laterality::{latch_rails, occupied_band_fill, Pair, Rhythm, Side, WINDOW_SEC};
use crate::theme;
use crate::widgets::head_plot::{HeadOverlayFrame, HeadOverlayKind, OverlaySite};
use crate::widgets::mark_iv::{
    self, paint_frame_hiding_pair, paint_head, paint_labeled_inserts, project_holes, Camera,
    LabeledInsert, DEFAULT_SITES,
};
use crate::widgets::Widget;
use eframe::egui;

pub struct WHemispheres {
    title: String,
    selected_pair: Pair,
    selected_rhythm: Rhythm,
    railed: [bool; 8],
    channel_psd: [[f64; 5]; 8],
    orbit: Camera,
}

impl WHemispheres {
    pub fn new() -> Self {
        Self {
            title: "Left / right".to_string(),
            selected_pair: Pair::O1O2,
            selected_rhythm: Rhythm::Alpha,
            railed: [false; 8],
            channel_psd: [[0.0; 5]; 8],
            orbit: Camera::default(),
        }
    }

    pub fn overlay_frame(&self) -> HeadOverlayFrame {
        // Glass caption is rest-only from O1 vs O2 alpha power. No pair name, no band letter, no Hz.
        let band = Rhythm::Alpha.psd_index();
        let o1 = if self.railed[crate::laterality::IDX_O1] {
            0.0
        } else {
            self.channel_psd[crate::laterality::IDX_O1][band]
        };
        let o2 = if self.railed[crate::laterality::IDX_O2] {
            0.0
        } else {
            self.channel_psd[crate::laterality::IDX_O2][band]
        };
        let caption = if o1 > o2 {
            "Left rest is louder".to_string()
        } else {
            "Right rest is louder".to_string()
        };
        // Fill is each channel's power in the overlay band — all 8 live inserts, not only O1/O2.
        let fill_band = self.selected_rhythm.psd_index();
        let mut psd = [0.0_f64; 8];
        for i in 0..8 {
            if !self.railed[i] {
                psd[i] = self.channel_psd[i][fill_band];
            }
        }
        let fill = occupied_band_fill(&psd);
        let sites = (0..8)
            .map(|idx| OverlaySite {
                idx,
                fill: fill[idx],
            })
            .collect();
        HeadOverlayFrame {
            kind: HeadOverlayKind::Hemispheres,
            caption,
            sites,
            railed: self.railed,
        }
    }

    pub fn visible_sites(&self) -> Vec<OverlaySite> {
        let want = [
            self.selected_pair.left_idx(),
            self.selected_pair.right_idx(),
        ];
        self.overlay_frame()
            .sites
            .into_iter()
            .filter(|s| want.contains(&s.idx))
            .collect()
    }

    pub fn draw_sites(&self) -> Vec<OverlaySite> {
        self.visible_sites()
    }

    fn labeled_inserts(&self) -> Vec<LabeledInsert> {
        let vis = self.visible_sites();
        self.pair_names()
            .into_iter()
            .filter_map(|name| {
                let idx = DEFAULT_SITES.iter().position(|n| *n == name)?;
                let s = vis.iter().find(|s| s.idx == idx)?;
                Some(LabeledInsert {
                    name,
                    fill: s.fill,
                    railed: self.railed[idx],
                })
            })
            .collect()
    }

    fn pair_names(&self) -> [&'static str; 2] {
        [
            self.selected_pair.electrode(Side::Left),
            self.selected_pair.electrode(Side::Right),
        ]
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
            self.channel_psd = [[0.0; 5]; 8];
            return;
        }
        let n = (WINDOW_SEC * sr).round() as usize;
        let data = source.get_data(n.max(32));
        let exg = source.exg_channels();
        if exg.len() < 8 || data.is_empty() {
            self.channel_psd = [[0.0; 5]; 8];
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
        let raw_rows = source.get_raw_data(n.max(32));
        let mut raw_chs = Vec::new();
        for &col in exg.iter().take(8) {
            raw_chs.push(
                raw_rows
                    .iter()
                    .map(|row| row.get(col).copied().unwrap_or(0.0))
                    .collect::<Vec<f64>>(),
            );
        }
        latch_rails(&mut self.railed, &raw_chs);
        let railed = self.railed;
        self.channel_psd = [[0.0; 5]; 8];
        for (ch, samples) in channels.iter().enumerate().take(8) {
            if railed[ch] || samples.is_empty() {
                continue;
            }
            self.channel_psd[ch] = band_powers_psd(samples, sr);
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        _source: &dyn DataSource,
        _ctx: &mut crate::widget_context::WidgetContext,
    ) {
        ui.label(&self.overlay_frame().caption);
        ui.add_space(2.0);

        let avail = ui.available_size();
        let desired = egui::vec2(avail.x.max(40.0), (avail.y - 8.0).max(40.0));
        let (resp, painter) = ui.allocate_painter(desired, egui::Sense::drag());
        let rect = resp.rect;

        if resp.dragged() {
            self.orbit.drag(resp.drag_delta());
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
            let [a, b] = self.pair_names();
            paint_frame_hiding_pair(&painter, rect, self.orbit, a, b);
        }
        let projected = project_holes(rect, self.orbit);
        paint_labeled_inserts(&painter, rect, &projected, &self.labeled_inserts());
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
    use crate::fft::band_powers_psd;
    use crate::laterality::{IDX_C3, IDX_C4, IDX_FP1, IDX_O1, IDX_O2};

    fn sine(n: usize, sr: f64, hz: f64, amp: f64) -> Vec<f64> {
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / sr).sin())
            .collect()
    }

    fn tone_psd(ch: usize, hz: f64) -> [[f64; 5]; 8] {
        let sr = 250.0;
        let n = 512usize;
        let mut psd = [[0.0; 5]; 8];
        let mut chs = vec![vec![0.0; n]; 8];
        chs[ch] = sine(n, sr, hz, 10.0);
        for i in 0..8 {
            psd[i] = band_powers_psd(&chs[i], sr);
        }
        psd
    }

    fn fill_at(frame: &HeadOverlayFrame, idx: usize) -> f32 {
        frame
            .sites
            .iter()
            .find(|s| s.idx == idx)
            .map(|s| s.fill)
            .unwrap()
    }

    #[test]
    fn occupied_fill_uses_per_channel_band_not_only_the_overlay_pair() {
        let mut w = WHemispheres::new();
        w.channel_psd = tone_psd(IDX_FP1, 10.0);
        let frame = w.overlay_frame();
        assert!(
            frame.caption == "Left rest is louder" || frame.caption == "Right rest is louder",
            "{}",
            frame.caption
        );
        assert!(!frame.caption.contains('α'), "{}", frame.caption);
        assert!(!frame.caption.contains("Hz"), "{}", frame.caption);
        assert!(!frame.caption.contains("P3"), "{}", frame.caption);
        assert_eq!(frame.sites.len(), 8);
        assert!(
            fill_at(&frame, IDX_FP1) > 0.8,
            "Fp1 alpha must paint, sites={:?}",
            frame.sites
        );
        assert!(
            fill_at(&frame, IDX_O1) < 0.05 && fill_at(&frame, IDX_O2) < 0.05,
            "O1/O2 silent — not pair-only gold, sites={:?}",
            frame.sites
        );
        assert!(fill_at(&frame, IDX_C3) < 0.05);

        w.channel_psd = tone_psd(IDX_C3, 10.0);
        let frame = w.overlay_frame();
        assert!(fill_at(&frame, IDX_C3) > 0.8);
        assert!(fill_at(&frame, IDX_O1) < 0.05);
        assert!(!frame.caption.contains('α'));
        assert!(!frame.caption.contains("Hz"));
        assert!(
            frame.caption == "Left rest is louder" || frame.caption == "Right rest is louder",
            "{}",
            frame.caption
        );
    }

    #[test]
    fn eight_live_channels_with_band_power_all_get_discs() {
        let mut w = WHemispheres::new();
        let i = Rhythm::Alpha.psd_index();
        for ch in 0..8 {
            w.channel_psd[ch][i] = 1.0 + ch as f64 * 0.1;
        }
        let frame = w.overlay_frame();
        assert_eq!(frame.sites.len(), 8);
        for s in &frame.sites {
            assert!(s.fill > 0.3, "ch {} fill {}", s.idx, s.fill);
        }
    }

    #[test]
    fn railed_channel_stays_hollow() {
        let mut w = WHemispheres::new();
        let i = Rhythm::Alpha.psd_index();
        for ch in 0..8 {
            w.channel_psd[ch][i] = 4.0;
        }
        w.railed[2] = true;
        let frame = w.overlay_frame();
        assert_eq!(fill_at(&frame, 2), 0.0);
        assert!(frame.railed[2]);
        assert!(fill_at(&frame, IDX_O1) > 0.3);
    }

    #[test]
    fn empty_insert_o1_is_not_in_sites() {
        use crate::widgets::mark_iv::DEFAULT_SITES;
        let mut w = WHemispheres::new();
        let i = Rhythm::Alpha.psd_index();
        for ch in 0..8 {
            w.channel_psd[ch][i] = 1.0;
        }
        let frame = w.overlay_frame();
        assert_eq!(frame.sites.len(), 8);
        assert!(frame.sites.iter().all(|s| s.fill > 0.0));
        assert!(DEFAULT_SITES.contains(&"O1"));
        assert!(!DEFAULT_SITES.contains(&"P3"));
        assert!(frame.sites.iter().all(|s| s.idx < 8));
        let hp = crate::widgets::head_plot::WHeadPlot::new();
        assert!(hp
            .channel_map()
            .iter()
            .any(|n| n.eq_ignore_ascii_case("O1")));
        assert!(!hp
            .channel_map()
            .iter()
            .any(|n| n.eq_ignore_ascii_case("P3")));
    }

    #[test]
    fn visible_sites_are_the_caption_pair_only() {
        let mut w = WHemispheres::new();
        let vis = w.visible_sites();
        assert_eq!(vis.len(), 2, "{vis:?}");
        assert_eq!(w.draw_sites().len(), 2);
        let mut idxs: Vec<usize> = vis.iter().map(|s| s.idx).collect();
        idxs.sort();
        assert_eq!(idxs, vec![IDX_O1, IDX_O2]);
        assert_eq!(w.overlay_frame().sites.len(), 8);
        let names = w.pair_names();
        assert_eq!(names, ["O1", "O2"]);
        let labs = w.labeled_inserts();
        assert_eq!(labs.len(), 2);
        let lab_names: Vec<&str> = labs.iter().map(|l| l.name).collect();
        assert!(lab_names.contains(&"O1") && lab_names.contains(&"O2"));
        assert!(!lab_names.contains(&"Fp1"));
        assert!(!lab_names.contains(&"C3"));

        w.selected_pair = Pair::C3C4;
        let vis = w.visible_sites();
        assert_eq!(vis.len(), 2);
        let mut idxs: Vec<usize> = vis.iter().map(|s| s.idx).collect();
        idxs.sort();
        assert_eq!(idxs, vec![IDX_C3, IDX_C4]);
        assert_eq!(w.overlay_frame().sites.len(), 8);
        assert!(!vis.iter().any(|s| s.idx == IDX_O1 || s.idx == IDX_FP1));
    }

    #[test]
    fn pair_view_paints_mark_iv_not_the_list() {
        let src = include_str!("hemispheres.rs");
        assert!(src.contains("paint_head"));
        assert!(src.contains("paint_frame"));
        assert!(src.contains("paint_frame_hiding_pair"));
        assert!(src.contains("project_holes"));
        assert!(src.contains("paint_labeled_inserts"));
        assert!(src.contains("visible_sites"));
        assert!(!src.contains(concat!("Power ratio on named", " pairs")));
        assert!(!src.contains(concat!("Self", "-test")));
        assert!(!src.contains(concat!("Contamination", " (scoring")));
        assert!(!src.contains(concat!("paint_li", "_bar")));
        assert!(!src.contains(concat!("Waiting for 8ch", " EXG")));
        assert!(src.contains("Left rest is louder"));
        assert!(src.contains("Right rest is louder"));
    }

    #[test]
    fn glass_caption_is_rest_only_from_o1_o2_alpha() {
        let mut w = WHemispheres::new();
        let i = Rhythm::Alpha.psd_index();
        w.channel_psd[IDX_O1][i] = 4.0;
        w.channel_psd[IDX_O2][i] = 1.0;
        assert_eq!(w.overlay_frame().caption, "Left rest is louder");
        w.channel_psd[IDX_O1][i] = 1.0;
        w.channel_psd[IDX_O2][i] = 4.0;
        let c = w.overlay_frame().caption;
        assert_eq!(c, "Right rest is louder");
        assert!(!c.contains('α') && !c.contains("Hz") && !c.contains("O1"));
    }
}
