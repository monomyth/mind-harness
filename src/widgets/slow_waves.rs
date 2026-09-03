//! Which first pane: Mark IV, selected pair's two holes + lag caption. Not a path.

use crate::board::DataSource;
use crate::fft::band_psd_excluding;
use crate::laterality::latch_rails;
use crate::slow_waves::{analyze_window, Band, LagResult, WINDOW_SEC};
use crate::theme;
use crate::widgets::head_plot::{HeadOverlayFrame, HeadOverlayKind, OverlaySite};
use crate::widgets::mark_iv::{
    self, paint_frame_hiding_pair, paint_head, paint_labeled_inserts, project_holes, Camera,
    LabeledInsert,
};
use crate::widgets::Widget;
use eframe::egui;

pub struct WSlowWaves {
    title: String,
    band: Band,
    results: Vec<LagResult>,
    selected_pair: crate::slow_waves::Pair,
    railed: [bool; 8],
    channel_power: [f64; 8],
    orbit: Camera,
}

impl WSlowWaves {
    pub fn new() -> Self {
        Self {
            title: "Which first".to_string(),
            band: Band::Slow,
            results: Vec::new(),
            selected_pair: crate::slow_waves::Pair::O1O2,
            railed: [false; 8],
            channel_power: [0.0; 8],
            orbit: Camera::default(),
        }
    }

    pub fn overlay_frame(&self) -> HeadOverlayFrame {
        use crate::laterality::occupied_band_fill;
        // Signed plate: O1/O2 lag sign only. No pair label, no band, no ms, no Hz.
        let lag = self
            .results
            .iter()
            .find(|r| r.pair == crate::slow_waves::Pair::O1O2)
            .map(|r| r.lag_ms)
            .unwrap_or(0.0);
        let caption = if lag < 0.0 {
            "O2 then O1".to_string()
        } else {
            "O1 then O2".to_string()
        };
        let fill = occupied_band_fill(&self.channel_power);
        HeadOverlayFrame {
            kind: HeadOverlayKind::SlowWaves,
            caption,
            sites: (0..8)
                .map(|idx| OverlaySite {
                    idx,
                    fill: fill[idx],
                    hz: None,
                })
                .collect(),
            railed: self.railed,
        }
    }

    pub fn visible_sites(&self) -> Vec<OverlaySite> {
        let want = self.selected_pair.draw_idx();
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
        self.selected_pair
            .draw_idx()
            .into_iter()
            .filter_map(|idx| {
                let name = *crate::widgets::mark_iv::DEFAULT_SITES.get(idx)?;
                let s = vis.iter().find(|s| s.idx == idx)?;
                Some(LabeledInsert {
                    name,
                    fill: s.fill,
                    railed: self.railed[idx],
                })
            })
            .collect()
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
            self.channel_power = [0.0; 8];
            return;
        }
        let n = (WINDOW_SEC * sr).round() as usize;
        let data = source.get_data(n.max(32));
        let exg = source.exg_channels();
        if exg.len() < 8 || data.is_empty() {
            self.results.clear();
            self.channel_power = [0.0; 8];
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
        let (lo, hi) = self.band.hz();
        let mut power = [0.0f64; 8];
        for (ch, samples) in channels.iter().enumerate().take(8) {
            if samples.is_empty() || self.railed[ch] {
                continue;
            }
            power[ch] = band_psd_excluding(samples, sr, lo, hi, None);
        }
        self.channel_power = power;
        self.results = analyze_window(&channels, sr, self.band);
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
            let [a, b] = self.selected_pair.site_names();
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
    use crate::slow_waves::Pair;
    use crate::widgets::mark_iv::DEFAULT_SITES;

    #[test]
    fn occupied_fill_uses_per_channel_slow_metric_not_only_pair() {
        let mut w = WSlowWaves::new();
        w.channel_power = [0.50, 0.40, 0.35, 0.30, 0.55, 0.60, 0.20, 0.90];
        let frame = w.overlay_frame();
        assert_eq!(frame.sites.len(), 8);
        assert!(
            frame.sites.iter().all(|s| s.fill > 0.0),
            "sites={:?}",
            frame.sites
        );
        assert!(
            frame.caption == "O1 then O2" || frame.caption == "O2 then O1",
            "{}",
            frame.caption
        );
        assert!(!frame.caption.contains('α'), "{}", frame.caption);
        assert!(!frame.caption.contains("Hz"), "{}", frame.caption);
        assert!(!frame.caption.contains("ms"), "{}", frame.caption);
        let fp1 = frame.sites.iter().find(|s| s.idx == 0).unwrap().fill;
        let o2 = frame.sites.iter().find(|s| s.idx == 7).unwrap().fill;
        assert!(o2 > fp1, "O2 louder than Fp1: {fp1} vs {o2}");
    }

    #[test]
    fn eight_live_slow_channels_all_get_discs() {
        let mut w = WSlowWaves::new();
        w.channel_power = [1.0; 8];
        let frame = w.overlay_frame();
        assert_eq!(frame.sites.len(), 8);
        assert_eq!(frame.sites.iter().filter(|s| s.fill > 0.02).count(), 8);
    }

    #[test]
    fn empty_insert_p3_is_not_default_occupied() {
        let mut w = WSlowWaves::new();
        w.channel_power = [0.4; 8];
        let frame = w.overlay_frame();
        assert!(DEFAULT_SITES.contains(&"O1"));
        assert!(DEFAULT_SITES.contains(&"O2"));
        assert!(!DEFAULT_SITES.contains(&"P3"));
        assert!(!DEFAULT_SITES.contains(&"F7"));
        assert_eq!(frame.sites.len(), 8);
        assert!(frame.sites.iter().all(|s| s.idx < 8));
    }

    #[test]
    fn visible_sites_are_the_selected_pair_only() {
        let mut w = WSlowWaves::new();
        let vis = w.visible_sites();
        assert_eq!(vis.len(), 2, "{vis:?}");
        assert_eq!(w.draw_sites().len(), 2);
        let mut idxs: Vec<usize> = vis.iter().map(|s| s.idx).collect();
        idxs.sort();
        assert_eq!(idxs, vec![6, 7]);
        assert_eq!(w.overlay_frame().sites.len(), 8);
        let labs = w.labeled_inserts();
        assert_eq!(labs.len(), 2);
        let names: Vec<&str> = labs.iter().map(|l| l.name).collect();
        assert!(names.contains(&"O1") && names.contains(&"O2"));
        assert!(!names.contains(&"Fp1"));
        assert!(!names.contains(&"C3"));

        w.selected_pair = Pair::AnteriorPosterior;
        let vis = w.visible_sites();
        assert_eq!(vis.len(), 2, "{vis:?}");
        let mut idxs: Vec<usize> = vis.iter().map(|s| s.idx).collect();
        idxs.sort();
        assert_eq!(idxs, vec![0, 6]);
        assert_eq!(w.overlay_frame().sites.len(), 8);
        let labs = w.labeled_inserts();
        let names: Vec<&str> = labs.iter().map(|l| l.name).collect();
        assert!(names.contains(&"Fp1") && names.contains(&"O1"));
        assert!(!names.contains(&"C3"));
        assert!(!names.contains(&"C4"));
        assert!(!names.contains(&"Fp2"));
        assert!(!names.contains(&"O2"));
    }

    #[test]
    fn pair_view_paints_mark_iv_not_the_list() {
        let src = include_str!("slow_waves.rs");
        assert!(src.contains("paint_head"));
        assert!(src.contains("paint_frame"));
        assert!(src.contains("paint_frame_hiding_pair"));
        assert!(src.contains("project_holes"));
        assert!(src.contains("paint_labeled_inserts"));
        assert!(src.contains("visible_sites"));
        assert!(!src.contains(concat!("Slow waves (pairwise", " lag, not a path)")));
        assert!(!src.contains(concat!("Self", "-test")));
        assert!(!src.contains(concat!("slow_wave", "_band")));
        assert!(!src.contains(concat!("Waiting for 8ch", " EXG")));
        assert!(!src.contains(concat!("human", "_copy")));
        assert!(src.contains("O1 then O2"));
        assert!(src.contains("O2 then O1"));
    }

    #[test]
    fn glass_caption_is_then_order_from_lag_sign() {
        use crate::slow_waves::{Band, LagResult};
        let mut w = WSlowWaves::new();
        assert_eq!(w.selected_pair, Pair::O1O2);
        w.results = vec![LagResult {
            band: Band::Slow,
            pair: Pair::O1O2,
            lag_ms: 12.0,
            direction: None,
            conf: 0.0,
        }];
        assert_eq!(w.overlay_frame().caption, "O1 then O2");
        w.results[0].lag_ms = -8.0;
        assert_eq!(w.overlay_frame().caption, "O2 then O1");
        let c = w.overlay_frame().caption;
        assert!(!c.contains('α') && !c.contains("Hz") && !c.contains("ms"));
    }
}
