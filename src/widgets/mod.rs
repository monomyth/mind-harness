//! Widget system for the OpenBCI GUI Rust port.
//!
//! This is the foundation for the flexible container-based widget architecture
//! that made the original Java GUI so powerful.

pub mod accelerometer;
pub mod analog;
pub mod band_power;
pub mod digital;
pub mod emg;
pub mod emg_joystick;
pub mod fft;
pub mod focus;
pub mod hardware_settings;
pub mod head_plot;
pub mod hemispheres;
pub mod impedance;
pub mod marker;
pub mod networking;
pub mod pulse;
pub mod slow_waves;
pub mod spectrogram;
pub mod time_series;

pub use accelerometer::WAccelerometer;
pub use analog::WAnalogRead;
pub use band_power::WBandPower;
pub use digital::WDigitalRead;
pub use emg::WEmg;
pub use emg_joystick::WEmgJoystick;
pub use fft::WFFT;
pub use focus::WFocus;
pub use hardware_settings::WHardwareSettings;
pub use head_plot::WHeadPlot;
pub use hemispheres::WHemispheres;
pub use impedance::WImpedance;
pub use marker::WMarker;
pub use networking::WNetworking;
pub use pulse::WPulseSensor;
pub use slow_waves::WSlowWaves;
pub use spectrogram::WSpectrogram;
pub use time_series::WTimeSeries;

use crate::board::DataSource;

/// Smoothing factors for temporal exponential averaging (FFT, BandPower, etc.).
/// Matches the original Java GUI "Smooth" dropdown values exactly (0.0 = no smoothing, 0.999 = very heavy).
pub const SMOOTH_FACTORS: &[f32] = &[0.0, 0.5, 0.75, 0.9, 0.95, 0.98, 0.99, 0.999];

/// Disable pan / zoom / boxed-zoom / scroll. EEG plots are read-only views.
pub fn lock_plot_interaction(plot: egui_plot::Plot<'_>) -> egui_plot::Plot<'_> {
    plot.allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .sense(egui::Sense::hover())
}

/// Tick text that stays valid for large steps (the egui_plot default uses
/// `-log10(step) as usize`, which wraps when step ≥ 10 and hides labels).
pub fn axis_tick_label(value: f64) -> String {
    if value.abs() >= 10.0 || (value - value.round()).abs() < 1e-6 {
        format!("{:.0}", value)
    } else if value.abs() >= 1.0 {
        format!("{:.1}", value)
    } else {
        format!("{:.2}", value)
    }
}

/// Core trait for all GUI widgets.
///
/// In the original Java GUI, widgets are placed inside "containers" that can be
/// rearranged in different layouts. This trait is the Rust equivalent.
pub trait Widget {
    /// Human-readable title shown in the widget header / dropdown.
    fn title(&self) -> &str;

    /// Called every frame to let the widget pull/process the latest data.
    /// Most widgets will read from the `DataSource` here.
    fn update(&mut self, source: &dyn DataSource);

    /// Draw the widget into the given egui `Ui` region.
    /// The caller is responsible for giving the widget a properly sized rectangle.
    ///
    /// `ctx` gives the widget the ability to trigger side-effects (send markers,
    /// reconfigure networking, etc.) without the widget needing a direct &mut
    /// reference to the whole app. This is the enabler for Phase 4+ interactivity.
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn DataSource,
        ctx: &mut crate::widget_context::WidgetContext,
    );

    /// Optional: called when the window is resized.
    /// Phase 7: hook for future resizable hybrid layout / multi-monitor support (currently unused).
    #[allow(dead_code)]
    fn on_resize(&mut self, _new_width: f32, _new_height: f32) {}

    /// Support for downcasting (used for persistence snapshot of widget-specific settings).
    #[allow(dead_code)]
    fn as_any(&self) -> &dyn std::any::Any;
    #[allow(dead_code)]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[cfg(test)]
mod tests {
    use super::axis_tick_label;

    #[test]
    fn large_db_steps_still_format() {
        assert_eq!(axis_tick_label(-80.0), "-80");
        assert_eq!(axis_tick_label(-20.0), "-20");
        assert_eq!(axis_tick_label(0.0), "0");
        assert_eq!(axis_tick_label(60.0), "60");
    }
}
