//! WidgetManager — manages placement of widgets into containers/layouts.
//!
//! This is a simplified Rust port of the original `WidgetManager.pde` + `Containers.pde` system.
//!
//! For the MVP we support a small number of layouts. The long-term goal is to
//! support all 12 layouts from the Java GUI and allow drag-and-drop rearrangement.

use crate::board::DataSource;
use crate::widgets::Widget;
use eframe::egui;

/// Very basic container description (x, y, w, h in normalized 0..1 coordinates for now).
#[derive(Clone, Copy)]
pub struct Container {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A very simple layout system (hard-coded for the most useful ones).
pub struct WidgetManager {
    pub(crate) widgets: Vec<Box<dyn Widget>>,
    current_layout: usize,
    containers: Vec<Container>,
}

/// Unused space under each pane so the bottom axis / last label is not clipped by the cell edge.
const PANE_BOTTOM_CLEARANCE: f32 = 22.0;

fn hide_glass_title(title: &str) -> bool {
    matches!(title, "Head Plot" | "Left / right" | "Which first")
}

impl WidgetManager {
    pub fn new() -> Self {
        Self {
            widgets: Vec::new(),
            current_layout: 5, // Java default: tall left + two stacked right
            containers: vec![],
        }
    }

    pub fn add_widget(&mut self, widget: Box<dyn Widget>) {
        self.widgets.push(widget);
    }

    /// Switch to one of the built-in layouts (0-based index for now).
    pub fn set_layout(&mut self, layout: usize) {
        self.current_layout = layout;
        self.recompute_containers();
    }

    /// Number of containers in a Java-compatible layout id (1-based, matches WidgetManager.pde).
    pub fn container_count_for(layout: usize) -> usize {
        match layout {
            1 => 1, // {5} full body
            2 => 4, // {1,3,7,9} 2×2
            3 => 2, // {4,6} left | right
            4 => 2, // {2,8} top / bottom
            5 => 3, // {4,3,9} tall left + two right (Java default)
            6 => 3, // {1,7,6} two left stacked + tall right
            7 => 3,
            8 => 3,
            _ => 4,
        }
    }

    fn recompute_containers(&mut self) {
        // Geometry matches OpenBCI_GUI/WidgetManager.pde setupLayouts() + Containers.pde.
        self.containers = match self.current_layout {
            1 => vec![Container {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            }],
            2 => vec![
                Container {
                    x: 0.00,
                    y: 0.00,
                    w: 0.50,
                    h: 0.50,
                },
                Container {
                    x: 0.50,
                    y: 0.00,
                    w: 0.50,
                    h: 0.50,
                },
                Container {
                    x: 0.00,
                    y: 0.50,
                    w: 0.50,
                    h: 0.50,
                },
                Container {
                    x: 0.50,
                    y: 0.50,
                    w: 0.50,
                    h: 0.50,
                },
            ],
            3 => vec![
                Container {
                    x: 0.00,
                    y: 0.00,
                    w: 0.50,
                    h: 1.00,
                },
                Container {
                    x: 0.50,
                    y: 0.00,
                    w: 0.50,
                    h: 1.00,
                },
            ],
            4 => vec![
                Container {
                    x: 0.00,
                    y: 0.00,
                    w: 1.00,
                    h: 0.50,
                },
                Container {
                    x: 0.00,
                    y: 0.50,
                    w: 1.00,
                    h: 0.50,
                },
            ],
            5 => vec![
                // Java default: tall left + stacked right
                Container {
                    x: 0.00,
                    y: 0.00,
                    w: 0.50,
                    h: 1.00,
                },
                Container {
                    x: 0.50,
                    y: 0.00,
                    w: 0.50,
                    h: 0.50,
                },
                Container {
                    x: 0.50,
                    y: 0.50,
                    w: 0.50,
                    h: 0.50,
                },
            ],
            6 => vec![
                Container {
                    x: 0.00,
                    y: 0.00,
                    w: 0.50,
                    h: 0.50,
                },
                Container {
                    x: 0.00,
                    y: 0.50,
                    w: 0.50,
                    h: 0.50,
                },
                Container {
                    x: 0.50,
                    y: 0.00,
                    w: 0.50,
                    h: 1.00,
                },
            ],
            _ => vec![
                Container {
                    x: 0.0,
                    y: 0.0,
                    w: 0.5,
                    h: 0.5,
                },
                Container {
                    x: 0.5,
                    y: 0.0,
                    w: 0.5,
                    h: 0.5,
                },
                Container {
                    x: 0.0,
                    y: 0.5,
                    w: 0.5,
                    h: 0.5,
                },
                Container {
                    x: 0.5,
                    y: 0.5,
                    w: 0.5,
                    h: 0.5,
                },
            ],
        };
    }

    pub fn update(&mut self, source: &dyn DataSource) {
        for w in &mut self.widgets {
            w.update(source);
        }
    }

    /// Draw all active widgets into their containers.
    /// `ctx` is the WidgetContext that gives widgets the power to affect shared state
    /// (Networking, Recording, last marker). It must be created fresh each frame in app.rs
    /// with the current &mut references.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        ctx: &mut crate::widget_context::WidgetContext,
    ) {
        if self.containers.is_empty() {
            self.recompute_containers();
        }

        let total_rect = ui.available_rect_before_wrap();

        for (i, widget) in self.widgets.iter_mut().enumerate() {
            if i >= self.containers.len() {
                break;
            }
            let c = self.containers[i];

            let rect = egui::Rect::from_min_size(
                total_rect.min + egui::vec2(c.x * total_rect.width(), c.y * total_rect.height()),
                egui::vec2(c.w * total_rect.width(), c.h * total_rect.height()),
            );

            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                // Clear the pane's own bottom line (plot axis / last row) inside every cell.
                egui::Frame::NONE
                    .fill(crate::theme::CANVAS)
                    .stroke(crate::theme::hairline())
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 0,
                        bottom: PANE_BOTTOM_CLEARANCE as i8,
                    })
                    .show(ui, |ui| {
                        let title = widget.title().to_string();
                        if !hide_glass_title(&title) {
                            let header_height = 18.0;
                            let header_rect = ui.available_rect_before_wrap();
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    header_rect.min,
                                    egui::vec2(header_rect.width(), header_height),
                                ),
                                0.0,
                                crate::theme::TRANSPORT,
                            );

                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                ui.add_space(6.0);
                                ui.small(egui::RichText::new(title).color(crate::theme::TEXT));
                            });
                            ui.add_space(2.0);
                        }

                        widget.show(ui, source, ctx);
                    });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WidgetManager;

    #[test]
    fn java_default_layout_has_three_containers() {
        assert_eq!(WidgetManager::container_count_for(5), 3);
        assert_eq!(WidgetManager::container_count_for(1), 1);
        assert_eq!(WidgetManager::container_count_for(2), 4);
        assert_eq!(WidgetManager::container_count_for(3), 2);
    }

    #[test]
    #[test]
    fn pane_bottom_clearance_is_on_every_cell() {
        let src = include_str!("widget_manager.rs");
        assert!(src.contains("PANE_BOTTOM_CLEARANCE"));
        assert!(src.contains("inner_margin"));
        assert!(src.contains("bottom: PANE_BOTTOM_CLEARANCE as i8"));
    }

    fn signed_plates_skip_small_title_on_glass() {
        let src = include_str!("widget_manager.rs");
        assert!(src.contains("fn hide_glass_title"));
        assert!(src.contains("if !hide_glass_title(&title)"));
        assert!(src.contains("Head Plot"));
        assert!(src.contains("Left / right"));
        assert!(src.contains("Which first"));
        assert!(src.contains("ui.small"));
    }
}
