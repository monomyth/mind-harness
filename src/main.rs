// OpenBCI GUI - Native macOS Rewrite
// Entry point for the eframe application.
//
// This is the very first bootstrap for the Rust port of the OpenBCI GUI.
// Phase 0 goal: produce a working macOS .app that launches and shows a basic window.

mod app;
mod board;
mod control_panel;
mod data_logger;
mod data_writers;
mod emg;
mod event_log;
mod fft;
mod filter_settings;
mod networking;
mod stream_stats;
mod theme;
mod widget_context;
mod widget_manager;
mod widgets;

use app::OpenBciGuiApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    // Initialize tracing (similar to Java's CustomOutputStream + ConsoleLog)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("OpenBCI GUI (Rust) starting...");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([980.0, 580.0])
            .with_title(format!("OpenBCI GUI  v{}", env!("CARGO_PKG_VERSION")))
            // macOS: request a native-looking titlebar + vibrancy where possible
            .with_decorations(true)
            .with_transparent(false),
        // Use wgpu renderer (Metal on macOS) - best performance for real-time plots
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "OpenBCI GUI (Rust)",
        native_options,
        Box::new(|cc| {
            // Phase 7: Embed professional fonts from the original OpenBCI GUI (Java)
            // Montserrat for headings, OpenSans for body — makes the Rust port look
            // polished and consistent with the reference implementation.
            let mut fonts = egui::FontDefinitions::default();

            // Embed the .otf / .ttf bytes directly (no external files at runtime)
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

            // Make Montserrat the primary for proportional text (headings feel premium)
            // Phase 7 polish: defensive access (egui defaults always have the families, but we
            // avoid unwrap in case of future egui changes — graceful fallback to built-in fonts).
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
