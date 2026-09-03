//! Mind Harness library — shared by the GUI binary and headless cyton-probe.

pub mod app;
pub mod board;
pub mod contact;
pub mod control_panel;
pub mod data_logger;
pub mod data_writers;
pub mod emg;
pub mod event_log;
pub mod experiment;
pub mod export;
pub mod fft;
pub mod filter_settings;
pub mod laterality;
pub mod markers;
pub mod montage;
pub mod networking;
pub mod slow_waves;
pub mod spikes;
pub mod starve;
pub mod stream_stats;
pub mod theme;
pub mod widget_context;
pub mod widget_manager;
pub mod widgets;

#[cfg(test)]
mod test_support;

use app::OpenBciGuiApp;
use eframe::egui;

pub fn run() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Mind Harness starting...");

    let icon = egui::IconData {
        rgba: include_bytes!("../resources/mind-harness-icon.rgba").to_vec(),
        width: 256,
        height: 256,
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([980.0, 580.0])
            .with_title(format!("Mind Harness  v{}", env!("CARGO_PKG_VERSION")))
            .with_icon(icon)
            .with_decorations(true)
            .with_transparent(false),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Mind Harness",
        native_options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            let montserrat = include_bytes!("../resources/fonts/Montserrat-Regular.otf");
            let opensans = include_bytes!("../resources/fonts/OpenSans-Regular.ttf");

            fonts.font_data.insert(
                "Montserrat".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(montserrat)),
            );
            fonts.font_data.insert(
                "OpenSans".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(opensans)),
            );

            if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                proportional.insert(0, "Montserrat".to_owned());
            }
            if let Some(monospace) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                monospace.insert(0, "OpenSans".to_owned());
            }

            cc.egui_ctx.set_fonts(fonts);
            crate::theme::apply_visuals(&cc.egui_ctx);

            Ok(Box::new(OpenBciGuiApp::new(cc)))
        }),
    )
}
