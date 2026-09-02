// OpenBCI GUI Application State
//
// Now driven by the Widget + WidgetManager system.

use crate::board::ads_settings::{default_bank, AdsChannel};
use crate::board::brainflow_board::BrainFlowBoard;
use crate::board::{extract_exg, recent_raw_rows, DataSource};
use crate::control_panel::{ControlPanel, DataSourceType};
use crate::data_logger::DataLogger;
use crate::event_log::EventLog;
use crate::filter_settings::NotchMode;
use crate::montage::MontageStore;
use crate::networking::{NetworkingManager, Protocol};
use crate::theme;
use crate::widget_context::WidgetContext;
use crate::widget_manager::WidgetManager;
use crate::widgets::{
    WAccelerometer, WAnalogRead, WBandPower, WDigitalRead, WEmg, WEmgJoystick, WFocus,
    WHardwareSettings, WHeadPlot, WHemispheres, WImpedance, WMarker, WNetworking, WPulseSensor,
    WSlowWaves, WSpectrogram, WTimeSeries, Widget, WFFT,
};
use crate::widgets::head_plot::MontageUiAction;
use crate::widgets::mark_iv::HEADSET_NAME;
use directories::ProjectDirs;
use eframe::egui;
use std::collections::HashMap;
use tokio::sync::oneshot;

#[derive(Clone, Copy, PartialEq)]
pub enum SystemMode {
    PreInit,
    PostInit,
}

/// True while the PreInit setup panel should keep the frame (no running session yet).
/// Session start must flip to PostInit even when a PROPERTIES accordion section is open.
pub(crate) fn setup_panel_active(mode: SystemMode) -> bool {
    mode == SystemMode::PreInit
}

/// State while we are connecting to real hardware in the background.
enum ConnectionState {
    Idle,
    InProgress {
        receiver: oneshot::Receiver<Result<BrainFlowBoard, String>>,
        status_message: String,
    },
    Failed(String),
}

/// Phase 7 Reconnect support (plan.md Phase 7 polish / step 8).
/// Stores enough state from the ControlPanel + successful connect so that after
/// a Failed (bad cable, dongle yank on macOS, etc.) the user can hit one button
/// and retry the *exact* same settings without re-clicking dropdowns.
#[derive(Clone, Debug)]
pub struct LastConnectionParams {
    pub source: DataSourceType,
    /// Store the *name* not the dropdown index — survives port list refresh.
    pub serial_port_name: Option<String>,
    pub channels: usize,
    pub playback_file: Option<String>,
}

/// Phase 8: simple persisted user settings (loaded silently on startup, saved on Start/End).
/// Uses directories + serde_json for cross-platform robust config (no new heavy deps).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct PersistedSettings {
    selected_source: DataSourceType,
    synthetic_channels: usize,
    cyton_channels: usize,
    last_serial_port: Option<String>,
    playback_file: Option<String>,
    recording_format: crate::data_logger::LogFormat,
    filter_notch_enabled: bool,
    #[serde(default)]
    filter_notch_mode: Option<NotchMode>,
    filter_bandpass_enabled: bool,
    #[serde(default = "default_bp_low")]
    filter_bandpass_low: f64,
    #[serde(default = "default_bp_high")]
    filter_bandpass_high: f64,
    udp_enabled: bool,
    udp_target: String,
    osc_enabled: bool,
    osc_target: String,
    #[serde(default)]
    lsl_enabled: bool,
    #[serde(default)]
    lsl_target: String,
    #[serde(default)]
    ads_channels: Vec<AdsChannel>,
    #[serde(default)]
    cyton_wifi_ip: String,
    #[serde(default)]
    ganglion_device_id: String,
    #[serde(default)]
    sd_file: Option<String>,

    // Post-Phase 8 polish: Graph speed & stability controls (Time Window + Smoothing wave)
    // + per-channel y-scale overrides for the classic ChannelBar +/- experience
    ts_time_window_sec: f32,
    ts_y_scale_uv: f32,
    fft_smoothing_index: usize,
    bp_smoothing_index: usize,
    #[serde(default)]
    ts_per_channel_y_scales: Vec<f32>,
    #[serde(default = "default_layout_id")]
    current_layout: usize,
    #[serde(default)]
    font_sizes: theme::FontSizes,
    #[serde(default = "default_true")]
    head_show_waves: bool,
    #[serde(default = "default_true")]
    head_show_hemispheres: bool,
}

fn default_layout_id() -> usize {
    5
}

fn default_true() -> bool {
    true
}

fn default_bp_low() -> f64 {
    crate::filter_settings::DEFAULT_BANDPASS_LOW
}

fn default_bp_high() -> f64 {
    crate::filter_settings::DEFAULT_BANDPASS_HIGH
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            selected_source: DataSourceType::Synthetic,
            synthetic_channels: 8,
            cyton_channels: 8,
            last_serial_port: None,
            playback_file: None,
            recording_format: crate::data_logger::LogFormat::BDF,
            filter_notch_enabled: true,
            filter_notch_mode: Some(NotchMode::FiftyAndSixty),
            filter_bandpass_enabled: true,
            filter_bandpass_low: crate::filter_settings::DEFAULT_BANDPASS_LOW,
            filter_bandpass_high: crate::filter_settings::DEFAULT_BANDPASS_HIGH,
            udp_enabled: false,
            udp_target: "127.0.0.1:12345".to_string(),
            osc_enabled: false,
            osc_target: "127.0.0.1:9000".to_string(),
            lsl_enabled: false,
            lsl_target: crate::networking::lsl_stream::DEFAULT_EEG_NAME.to_string(),
            ads_channels: vec![],
            cyton_wifi_ip: String::new(),
            ganglion_device_id: String::new(),
            sd_file: None,

            // Defaults chosen to match previous hard-coded behavior + good UX
            ts_time_window_sec: 5.0,
            ts_y_scale_uv: 200.0,
            fft_smoothing_index: 2, // 0.75
            bp_smoothing_index: 5,  // 0.98
            ts_per_channel_y_scales: vec![0.0; 16],
            current_layout: 5,
            font_sizes: theme::FontSizes::default(),
            head_show_waves: true,
            head_show_hemispheres: true,
        }
    }
}

pub struct OpenBciGuiApp {
    pub frame_count: u64,
    pub system_mode: SystemMode,
    pub board: Option<Box<dyn DataSource>>,
    pub streaming: bool,
    pub widget_manager: WidgetManager,
    pub current_layout: usize,

    /// Per-layout widget assignment for the central grid.
    /// Key = layout id, Value = ordered list of widget titles to show in that layout's containers.
    /// This is what allows the user to decide "when I pick layout X, show HeadPlot here, TimeSeries there".
    grid_layout_assignments: HashMap<usize, Vec<String>>,

    pending_layout_rebuild: bool,
    pub control_panel: ControlPanel,
    pub data_logger: DataLogger,
    pub networking: NetworkingManager,
    pub connection_status: String,
    pub recording_format: crate::data_logger::LogFormat,

    // Packet loss & sample rate tracking
    samples_received: u64,
    last_sample_time: Option<std::time::Instant>,
    current_sample_rate: f64,
    packet_loss_percent: f64,

    /// Phase 7: accumulator for samples delivered (via the new DataSource::recent_samples_delivered)
    /// within the current measurement window. Enables correct packet loss % for the SidePanel visual.
    window_samples: u64,
    window_lost: u64,

    // Last marker for status bar
    last_marker: String,

    // Phase 7: Central EventLog (feeds WConsole and the mini status log)
    event_log: EventLog,

    // Phase 7 Console UI state (filter + search + window toggle)
    console_search: String,
    console_categories: std::collections::HashSet<String>,
    console_show_window: bool,

    // Phase 7 hybrid layout (plan.md Phase 7 step 5): tool widgets live in a resizable right SidePanel.
    // This guarantees Marker, Networking, Focus (and later PacketLoss) are *always visible and usable*
    // next to the main visualization grid (TimeSeries/FFT/BandPower/Accel). Previously they were
    // populated into the manager but silently dropped because only 4 containers existed.
    // Console remains the global 📜 button + full filterable window (best UX for long audit log).
    tool_widgets: Vec<Box<dyn crate::widgets::Widget>>,

    // Phase 7: short ring history for packet loss sparkline (visual indicator in SidePanel)
    packet_loss_history: Vec<f32>,

    connection_state: ConnectionState,

    // Phase 7 Reconnect (plan.md Phase 7 final polish): last successful params for one-click retry
    // after Failed state (initial connect error or runtime dongle yank).
    last_connection: Option<LastConnectionParams>,

    /// Java `emgSettings.values` — updated every frame from filtered EXG.
    emg: crate::emg::EmgProcessor,

    // Phase 8: last-known global filter prefs (notch / bandpass) restored on new boards
    last_persisted_notch_mode: NotchMode,
    last_persisted_filter_bandpass: bool,
    last_persisted_filter_bandpass_low: f64,
    last_persisted_filter_bandpass_high: f64,

    // Post-Phase 8 "Finish the current wave": persisted graph speed/stability settings
    persisted_ts_time_window_sec: f32,
    persisted_ts_y_scale_uv: f32,
    persisted_fft_smoothing_index: usize,
    persisted_bp_smoothing_index: usize,
    persisted_ts_per_channel_y_scales: Vec<f32>,
    persisted_ads_channels: Vec<AdsChannel>,
    last_recording_path: Option<std::path::PathBuf>,

    /// Exclusive PROPERTIES accordion. None = Session open; Some(id) = that section open.
    properties_open: Option<String>,

    font_sizes: theme::FontSizes,
    head_show_waves: bool,
    head_show_hemispheres: bool,

    experiment: crate::experiment::ExperimentRun,
    contact: crate::contact::ContactLog,
    montage: MontageStore,
}

impl OpenBciGuiApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // current_layout must be set before populate so the helper knows which layout to use.
        // The populate call then creates the canonical 7-widget set (fix for the "missing widgets
        // after End Session / new session" bug reported in Phase 4 + Phase 6 reviews).
        let mut app = Self {
            frame_count: 0,
            system_mode: SystemMode::PreInit,
            board: None,
            streaming: false,
            widget_manager: WidgetManager::new(), // replaced immediately below
            current_layout: 5,

            grid_layout_assignments: {
                let mut m = HashMap::new();
                // Java-numbered layouts (WidgetManager.pde). Default is layout 5.
                m.insert(1, vec!["Time Series".into()]);
                m.insert(
                    2,
                    vec![
                        "Time Series".into(),
                        "FFT Plot".into(),
                        "Band Power".into(),
                        "Accelerometer".into(),
                    ],
                );
                m.insert(3, vec!["Time Series".into(), "Head Plot".into()]);
                m.insert(4, vec!["Time Series".into(), "FFT Plot".into()]);
                m.insert(
                    5,
                    vec!["Time Series".into(), "FFT Plot".into(), "Head Plot".into()],
                );
                m.insert(
                    6,
                    vec![
                        "Time Series".into(),
                        "FFT Plot".into(),
                        "Spectrogram".into(),
                    ],
                );
                m
            },
            pending_layout_rebuild: false,
            control_panel: ControlPanel::new(),
            data_logger: DataLogger::new(),
            networking: NetworkingManager::new(),
            connection_status: String::new(),
            recording_format: crate::data_logger::LogFormat::BDF,

            samples_received: 0,
            last_sample_time: None,
            current_sample_rate: 0.0,
            packet_loss_percent: 0.0,
            window_samples: 0,
            window_lost: 0,

            last_marker: String::new(),

            // Phase 7
            event_log: EventLog::new(),

            console_search: String::new(),
            console_categories: {
                let mut s = std::collections::HashSet::new();
                for c in [
                    "Marker",
                    "Recording",
                    "Networking",
                    "Connection",
                    "Focus",
                    "Filter",
                    "System",
                    "Error",
                ] {
                    s.insert(c.to_string());
                }
                s
            },
            console_show_window: false,

            tool_widgets: Vec::new(),

            packet_loss_history: Vec::with_capacity(60),

            connection_state: ConnectionState::Idle,

            // Phase 7 Reconnect
            last_connection: None,

            emg: crate::emg::EmgProcessor::new(8),

            // Phase 8 persistence defaults (overridden by load below)
            last_persisted_notch_mode: NotchMode::FiftyAndSixty,
            last_persisted_filter_bandpass: true,
            last_persisted_filter_bandpass_low: crate::filter_settings::DEFAULT_BANDPASS_LOW,
            last_persisted_filter_bandpass_high: crate::filter_settings::DEFAULT_BANDPASS_HIGH,

            // Will be overwritten by load_persisted_settings below
            persisted_ts_time_window_sec: 5.0,
            persisted_ts_y_scale_uv: 200.0,
            persisted_fft_smoothing_index: 2,
            persisted_bp_smoothing_index: 5, // 0.98
            persisted_ts_per_channel_y_scales: vec![0.0; 16],
            persisted_ads_channels: vec![],
            last_recording_path: None,
            properties_open: None,
            font_sizes: theme::FontSizes::default(),
            head_show_waves: true,
            head_show_hemispheres: true,
            experiment: crate::experiment::ExperimentRun::new(),
            contact: crate::contact::ContactLog::new(),
            montage: MontageStore::load(),
        };

        // === Phase 8: Load persisted settings (silent on any error / missing file) ===
        let persisted = OpenBciGuiApp::load_persisted_settings();
        app.control_panel.selected_source = persisted.selected_source;
        app.control_panel.synthetic_channels = persisted.synthetic_channels;
        app.control_panel.cyton_channels = persisted.cyton_channels;
        app.control_panel.playback_file = persisted.playback_file.clone();
        app.control_panel.cyton_wifi_ip = persisted.cyton_wifi_ip.clone();
        app.control_panel.ganglion_device_id = persisted.ganglion_device_id.clone();
        app.control_panel.sd_file = persisted.sd_file.clone();
        app.persisted_ads_channels = persisted.ads_channels.clone();
        app.font_sizes = persisted.font_sizes.clone();
        theme::set_font_sizes(app.font_sizes.clone());
        app.head_show_waves = persisted.head_show_waves;
        app.head_show_hemispheres = persisted.head_show_hemispheres;
        if let Some(ref name) = persisted.last_serial_port {
            if let Some(idx) = app
                .control_panel
                .serial_ports
                .iter()
                .position(|p| &p.port_name == name)
            {
                app.control_panel.selected_serial_port = Some(idx);
            }
        }
        app.recording_format = persisted.recording_format;
        app.last_persisted_notch_mode = persisted
            .filter_notch_mode
            .unwrap_or_else(|| NotchMode::from_legacy_enabled(persisted.filter_notch_enabled));
        app.last_persisted_filter_bandpass = persisted.filter_bandpass_enabled;
        app.last_persisted_filter_bandpass_low = persisted.filter_bandpass_low;
        app.last_persisted_filter_bandpass_high = persisted.filter_bandpass_high;

        // Load graph speed/stability settings (Time Window + Smoothing wave)
        app.persisted_ts_time_window_sec = persisted.ts_time_window_sec;
        app.persisted_ts_y_scale_uv = persisted.ts_y_scale_uv;
        app.persisted_fft_smoothing_index = persisted.fft_smoothing_index;
        app.persisted_bp_smoothing_index = persisted.bp_smoothing_index;
        app.persisted_ts_per_channel_y_scales = persisted.ts_per_channel_y_scales.clone();
        if persisted.current_layout >= 1 && persisted.current_layout <= 6 {
            app.current_layout = persisted.current_layout;
        }
        for cfg in &mut app.networking.configs {
            match cfg.protocol {
                Protocol::UDP => {
                    cfg.enabled = persisted.udp_enabled;
                    if !persisted.udp_target.is_empty() {
                        cfg.target = persisted.udp_target.clone();
                    }
                }
                Protocol::OSC => {
                    cfg.enabled = persisted.osc_enabled;
                    if !persisted.osc_target.is_empty() {
                        cfg.target = persisted.osc_target.clone();
                    }
                }
                Protocol::LSL => {
                    cfg.enabled = persisted.lsl_enabled
                        && crate::networking::NetworkingManager::lsl_available();
                    if !persisted.lsl_target.is_empty() {
                        cfg.target = persisted.lsl_target.clone();
                    }
                }
            }
        }
        // Phase 8: ensure persisted enabled networking targets have live senders ready for the upcoming session
        // (even though we are still in PreInit; harmless and makes first data push after Start work immediately).
        app.networking.apply_config();

        app.populate_widgets_for_new_session();
        app.populate_tool_widgets();
        app.apply_head_montage();
        if let Ok(path) = std::env::var("OPENBCI_PLAYBACK") {
            let seek: f32 = std::env::var("OPENBCI_PLAYBACK_SEEK_SEC")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            app.open_playback_file(&path, seek);
        }
        app.apply_recapture_layout_grid();
        app
    }

    fn apply_head_montage(&mut self) {
        self.apply_head_montage_inner(false);
    }

    fn apply_head_montage_inner(&mut self, force: bool) {
        let names = self.montage.names();
        let last = self.montage.last_name().to_string();
        let labels = self.montage.active().channel_labels();
        let holes = self.montage.active().channel_holes();
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(hp) = w.as_any_mut().downcast_mut::<WHeadPlot>() {
                hp.set_catalog(names.clone(), &last);
                if force || !hp.is_dirty() {
                    hp.set_profile_clean(labels.clone(), holes.clone(), &last);
                }
            }
            if let Some(ts) = w
                .as_any_mut()
                .downcast_mut::<crate::widgets::time_series::WTimeSeries>()
            {
                ts.set_channel_labels(labels.clone());
            }
        }
    }

    fn apply_head_plot_chrome(&mut self) {
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(hp) = w.as_any_mut().downcast_mut::<WHeadPlot>() {
                hp.show_waves = self.head_show_waves;
                hp.show_hemispheres = self.head_show_hemispheres;
            }
        }
    }

    fn sync_head_plot_chrome(&mut self) {
        let Some(hp) = self
            .widget_manager
            .widgets
            .iter()
            .chain(self.tool_widgets.iter())
            .find_map(|w| w.as_any().downcast_ref::<WHeadPlot>())
        else {
            return;
        };
        if hp.show_waves != self.head_show_waves
            || hp.show_hemispheres != self.head_show_hemispheres
        {
            self.head_show_waves = hp.show_waves;
            self.head_show_hemispheres = hp.show_hemispheres;
            self.save_current_persisted_settings();
        }
    }

    fn drain_head_montage(&mut self) {
        let mut action = None;
        let mut plots: Vec<([String; 8], bool)> = Vec::new();
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(hp) = w.as_any_mut().downcast_mut::<WHeadPlot>() {
                if action.is_none() {
                    action = hp.take_action();
                } else {
                    let _ = hp.take_action();
                }
                plots.push((hp.channel_holes(), hp.is_dirty()));
            }
        }
        let holes = pick_holes_for_montage_save(&plots, self.montage.active().channel_holes());
        match action {
            Some(MontageUiAction::Select(name)) => {
                self.montage.select(&name);
            }
            Some(MontageUiAction::Save) => {
                self.montage.save_active(holes);
            }
            Some(MontageUiAction::SaveAs(name)) => {
                self.montage.save_as(&name, holes);
            }
            None => return,
        }
        self.apply_head_montage_inner(true);
    }

    fn set_layout(&mut self, layout: usize) {
        self.current_layout = layout;
        self.rebuild_grid_widgets_for_current_layout();
    }

    /// Recapture-only: OPENBCI_LAYOUT=1..=6 and OPENBCI_GRID="Time Series,Head Plot,..."
    /// OPENBCI_ASSIGN_HOLE=C3 starts Head Plot with that hole chosen (crop cannot click).
    /// OPENBCI_PROPERTIES=Hardware opens that accordion section (requires OPENBCI_CROP).
    /// applied after playback boot so a crop can pin layout + slot titles without the UI.
    fn apply_recapture_layout_grid(&mut self) {
        if let Ok(s) = std::env::var("OPENBCI_LAYOUT") {
            if let Ok(n) = s.parse::<usize>() {
                if (1..=6).contains(&n) {
                    self.set_layout(n);
                }
            }
        }
        if let Ok(grid) = std::env::var("OPENBCI_GRID") {
            let titles: Vec<String> = grid
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            if !titles.is_empty() {
                let layout = self.current_layout;
                self.grid_layout_assignments.insert(layout, titles);
                self.rebuild_grid_widgets_for_current_layout();
            }
        }
        self.apply_recapture_assign_holes();
        self.apply_recapture_properties();
    }

    /// Recapture-only: OPENBCI_PROPERTIES=Hardware opens that Properties accordion section.
    /// Only takes effect when OPENBCI_CROP is set (crop scripts cannot click the spine).
    fn apply_recapture_properties(&mut self) {
        if std::env::var("OPENBCI_CROP").is_err() {
            return;
        }
        if let Ok(section) = std::env::var("OPENBCI_PROPERTIES") {
            let section = section.trim();
            if PROPERTIES_SPINE_IDS.contains(&section) {
                self.properties_open = Some(section.to_string());
            }
        }
    }

    /// Recapture-only: OPENBCI_ASSIGN_HOLE=C3 paints the chosen hole after montage wipe.
    fn apply_recapture_assign_holes(&mut self) {
        if std::env::var("OPENBCI_ASSIGN_HOLE").is_err() {
            return;
        }
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(hp) = w.as_any_mut().downcast_mut::<WHeadPlot>() {
                hp.apply_recapture_assign_hole();
            }
        }
    }

    /// Rebuilds the central grid widgets according to the saved assignment for the current layout.
    /// This is the core of "define what graph is displayed when I change layout".
    fn rebuild_grid_widgets_for_current_layout(&mut self) {
        let count = WidgetManager::container_count_for(self.current_layout);

        let assignment = self
            .grid_layout_assignments
            .entry(self.current_layout)
            .or_insert_with(|| {
                vec![
                    "Time Series".into(),
                    "FFT Plot".into(),
                    "Band Power".into(),
                    "Accelerometer".into(),
                ]
            });

        // Ensure correct length
        while assignment.len() < count {
            assignment.push("Time Series".into());
        }
        assignment.truncate(count);

        let mut wm = WidgetManager::new();

        for title in assignment.iter() {
            let widget: Box<dyn Widget> = match title.as_str() {
                "Time Series" => Box::new(WTimeSeries::new()),
                "FFT Plot" => Box::new(WFFT::new()),
                "Band Power" => Box::new(WBandPower::new()),
                "Accelerometer" => Box::new(WAccelerometer::new()),
                "Head Plot" => Box::new(WHeadPlot::new()),
                "Impedance" => Box::new(WImpedance::new()),
                "Spectrogram" => Box::new(WSpectrogram::new()),
                "EMG" => Box::new(WEmg::new()),
                "EMG Joystick" => Box::new(WEmgJoystick::new()),
                "Analog Read" => Box::new(WAnalogRead::new()),
                "Digital Read" => Box::new(WDigitalRead::new()),
                "Pulse Sensor" => Box::new(WPulseSensor::new()),
                "Board" => Box::new(WHardwareSettings::new()),
                "Left / right" => Box::new(WHemispheres::new()),
                "Which first" => Box::new(WSlowWaves::new()),
                _ => Box::new(WTimeSeries::new()),
            };
            wm.add_widget(widget);
        }

        wm.set_layout(self.current_layout);
        self.widget_manager = wm;
        self.apply_head_montage();
        self.apply_head_plot_chrome();
    }

    /// Create a fresh WidgetManager populated with the 4 core visualization widgets.
    /// These go into the main grid area (supports the classic layouts with up to 4 containers).
    /// Called from new() and every successful session start / end_session.
    /// Phase 7 hybrid: the interactive tool widgets (Focus, Networking, Marker, PacketLoss) are
    /// now in a dedicated SidePanel (see populate_tool_widgets) so they are never dropped.
    fn populate_widgets_for_new_session(&mut self) {
        self.emg.reset();
        self.contact.reset();
        // Use the per-layout assignment so the user controls exactly which graphs appear when they pick a layout.
        self.rebuild_grid_widgets_for_current_layout();

        // Re-apply persisted scale settings to whichever TimeSeries is currently in the grid.
        if let Some(ts) = self
            .widget_manager
            .widgets
            .iter_mut()
            .find_map(|w| w.as_any_mut().downcast_mut::<WTimeSeries>())
        {
            ts.set_time_window(self.persisted_ts_time_window_sec);
            ts.set_y_scale(self.persisted_ts_y_scale_uv);

            for (ch, &sc) in self
                .persisted_ts_per_channel_y_scales
                .iter()
                .enumerate()
                .take(16)
            {
                if sc > 0.0 {
                    ts.set_per_channel_y_scale(ch, sc);
                }
            }
        }
    }

    /// Shared tail for every successful session enter (live board, synthetic, or playback).
    fn enter_running_session(&mut self) {
        self.control_panel.show = false;
        self.populate_widgets_for_new_session();
        self.populate_tool_widgets();
        self.apply_persisted_filters_to_current_board();
        self.save_current_persisted_settings();
        self.system_mode = SystemMode::PostInit;
    }

    /// Phase 7 hybrid layout (plan.md Phase 7 step 5): populate the interactive tool widgets
    /// that live in the right SidePanel. These are always visible during a session.
    /// Focus (ML + audio) is now finally usable alongside the viz — the killer Phase 6 feature
    /// is no longer hidden. Marker and Networking controls are also always at hand.
    fn open_playback_file(&mut self, path: &str, seek_sec: f32) {
        match crate::board::playback::PlaybackBoard::from_file(std::path::Path::new(path)) {
            Ok(mut pb) => {
                let _ = pb.initialize();
                if pb.start_streaming().is_ok() {
                    self.streaming = true;
                }
                if seek_sec > 0.0 {
                    let total = pb.playback_progress().map(|(_, t)| t).unwrap_or(1).max(1) as f32;
                    let sr = pb.sample_rate().max(1) as f32;
                    pb.seek_to_fraction((seek_sec * sr / total).clamp(0.0, 1.0));
                }
                self.board = Some(Box::new(pb) as Box<dyn DataSource>);
                self.control_panel.playback_file = Some(path.to_string());
                self.current_layout = 1;
                self.grid_layout_assignments
                    .insert(1, vec!["Head Plot".into()]);
                self.pending_layout_rebuild = true;
                self.enter_running_session();
                self.connection_status = format!("Playback: {path}");
                self.event_log.log_connection(&format!(
                    "Playback · {}",
                    std::path::Path::new(&path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file"),
                ));
            }
            Err(e) => {
                self.event_log
                    .log_error(&format!("Playback boot failed: {e}"));
            }
        }
    }

    fn populate_tool_widgets(&mut self) {
        self.tool_widgets.clear();
        self.properties_open = None;
        self.tool_widgets.push(Box::new(WMarker::new()));
        self.tool_widgets.push(Box::new(WNetworking::new()));
        self.tool_widgets.push(Box::new(WFocus::new()));
        self.tool_widgets.push(Box::new(WHeadPlot::new()));
        self.tool_widgets.push(Box::new(WSlowWaves::new()));
        self.tool_widgets.push(Box::new(WHemispheres::new()));
        self.tool_widgets.push(Box::new(WImpedance::new()));
        self.tool_widgets.push(Box::new(WHardwareSettings::new()));
        self.tool_widgets.push(Box::new(WAnalogRead::new()));
        self.tool_widgets.push(Box::new(WDigitalRead::new()));
        self.tool_widgets.push(Box::new(WPulseSensor::new()));
        self.apply_head_montage();
        self.apply_head_plot_chrome();
        // NOTE: WPacketLoss is rendered inline in the SidePanel (sparkline + Reset + % ) — see the
        // "Phase 7 WPacketLoss visual" block near the tool loop. This keeps the visual right next
        // to the tools the user is looking at during an experiment; no separate widget object needed.
    }

    /// Gracefully end the current session and return to the Control Panel.
    /// This allows the user to switch boards (e.g. 8ch → 16ch Cyton) without restarting the app.
    fn end_session(&mut self) {
        // Snapshot live filter + widget state *before* dropping the board / resetting widgets.
        if let Some(ref b) = self.board {
            if let Some(ads) = b.ads_channels() {
                self.persisted_ads_channels = ads.to_vec();
            }
            if let Some(settings) = b.get_filter_settings() {
                if let Some(ch) = settings.channels.first() {
                    self.last_persisted_notch_mode = NotchMode::from_channel(ch);
                    self.last_persisted_filter_bandpass = ch.bandpass_enabled;
                }
            }
        }
        self.save_current_persisted_settings();

        if self.experiment.is_running() {
            self.cancel_experiment();
        }

        if self.data_logger.is_logging() {
            self.data_logger.stop();
            self.event_log
                .log_recording("Recording stopped (End Session)");
        }

        if let Some(mut board) = self.board.take() {
            if board.is_streaming() {
                let _ = board.stop_streaming();
            }
            let _ = board.uninitialize();
        }

        self.streaming = false;
        self.connection_status.clear();
        self.connection_state = ConnectionState::Idle;
        self.packet_loss_percent = 0.0;
        self.packet_loss_history.clear();
        self.window_samples = 0;
        self.window_lost = 0;
        self.last_sample_time = None;

        // Fresh viz + tools for next session (Phase 7 hybrid layout).
        self.populate_widgets_for_new_session();
        self.populate_tool_widgets();

        // Show Control Panel again
        self.control_panel.show = true;
        self.system_mode = SystemMode::PreInit;

        self.event_log
            .log_system("Session ended — returned to Control Panel (all widgets reset)");

        tracing::info!("Session ended — returned to Control Panel");
    }

    fn start_recording_like_session(&mut self) -> bool {
        let chans = self
            .board
            .as_ref()
            .map(|b| b.exg_channels().len())
            .unwrap_or(8);
        let sr = self.board.as_ref().map(|b| b.sample_rate()).unwrap_or(250);
        match self.data_logger.start(self.recording_format, chans, sr) {
            Ok(path) => {
                self.last_recording_path = Some(path.clone());
                self.connection_status = format!("Recording to {}", path.display());
                self.event_log.log_recording(&format!(
                    "Started {:?} → {}",
                    self.recording_format,
                    path.display()
                ));
                true
            }
            Err(e) => {
                self.connection_status = format!("Recording failed: {}", e);
                self.event_log
                    .log_error(&format!("Recording start failed: {}", e));
                false
            }
        }
    }

    fn stop_recording_like_session(&mut self) {
        if !self.data_logger.is_logging() {
            return;
        }
        if let Some(p) = self.data_logger.current_file() {
            self.last_recording_path = Some(p.clone());
        }
        self.data_logger.stop();
        self.connection_status.clear();
        self.event_log.log_recording("Recording stopped");
    }

    fn tick_contact_sidecar(&mut self) {
        let (chs, sample, t_s, sr_hz, loss_pct, markers, path) = {
            let Some(board) = self.board.as_deref() else {
                return;
            };
            let sr = board.sample_rate() as f64;
            if sr <= 1.0 {
                return;
            }
            let n = (2.0 * sr).round() as usize;
            let raw_rows = board.get_raw_data(n.max(32));
            let exg = board.exg_channels();
            let mut chs = Vec::new();
            for &col in exg.iter().take(8) {
                chs.push(
                    raw_rows
                        .iter()
                        .map(|row| row.get(col).copied().unwrap_or(0.0))
                        .collect::<Vec<f64>>(),
                );
            }
            if chs.is_empty() {
                return;
            }
            let is_playback = board.playback_progress().is_some();
            let sample = board.playhead_sample().unwrap_or_else(|| {
                if self.data_logger.is_logging() {
                    self.data_logger.samples_logged() as usize
                } else {
                    self.samples_received as usize
                }
            });
            let t_s = sample as f64 / sr.max(1.0);
            let sr_hz = if is_playback {
                sr
            } else if self.current_sample_rate > 1.0 {
                self.current_sample_rate
            } else {
                sr
            };
            let loss_pct = if is_playback {
                0.0
            } else {
                self.packet_loss_percent
            };
            let mut markers = board.session_markers().to_vec();
            if self.data_logger.is_logging() {
                for m in self.data_logger.markers() {
                    if !markers
                        .iter()
                        .any(|e| e.sample_index == m.sample_index && e.label == m.label)
                    {
                        markers.push(m.clone());
                    }
                }
            }
            let path = if self.data_logger.is_logging() {
                self.data_logger.current_file().cloned()
            } else if is_playback {
                self.control_panel
                    .playback_file
                    .as_ref()
                    .map(std::path::PathBuf::from)
            } else {
                None
            };
            (chs, sample, t_s, sr_hz, loss_pct, markers, path)
        };
        self.contact.observe(
            &chs,
            sample,
            t_s,
            sr_hz,
            loss_pct,
            &markers,
            path.as_deref(),
        );
    }

    fn write_experiment_marker(&mut self, label: &str) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let mut ctx = WidgetContext::new(
            &mut self.networking,
            &mut self.data_logger,
            &mut self.last_marker,
            &mut self.event_log,
            &mut self.emg,
        );
        ctx.send_marker(ts, label);
    }

    fn apply_experiment_event(&mut self, ev: crate::experiment::ExperimentEvent) {
        match ev {
            crate::experiment::ExperimentEvent::EnteredStep { index, label } => {
                crate::experiment::speak_detached(crate::experiment::STEPS[index].spoken);
                self.write_experiment_marker(&label);
                if index + 1 == crate::experiment::STEPS.len() {
                    self.stop_recording_like_session();
                }
            }
            crate::experiment::ExperimentEvent::Finished => {
                self.stop_recording_like_session();
            }
            crate::experiment::ExperimentEvent::Cancelled => {
                self.write_experiment_marker("Experiment cancelled");
                self.stop_recording_like_session();
            }
        }
    }

    fn start_experiment(&mut self) {
        if self.board.is_none() {
            self.connection_status = "Start a session first".to_string();
            return;
        }
        if !self.streaming {
            if let Some(ref mut b) = self.board {
                if let Err(e) = b.start_streaming() {
                    tracing::error!("Start failed: {:?}", e);
                    self.event_log
                        .log_error(&format!("Failed to start streaming: {}", e));
                    return;
                }
                self.streaming = true;
                self.event_log.log_system("Streaming started");
            }
        }
        if !self.data_logger.is_logging() && !self.start_recording_like_session() {
            return;
        }
        let ev = self.experiment.start(std::time::Instant::now());
        self.apply_experiment_event(ev);
    }

    fn cancel_experiment(&mut self) {
        if let Some(ev) = self.experiment.cancel() {
            self.apply_experiment_event(ev);
        }
    }

    fn tick_experiment(&mut self) {
        let now = std::time::Instant::now();
        while self.experiment.is_running() {
            match self.experiment.tick(now) {
                Some(ev) => self.apply_experiment_event(ev),
                None => break,
            }
        }
    }

    /// Phase 7 Reconnect (plan.md Phase 7 polish): snapshot the exact settings the user
    /// just successfully used. Called from every success path so that after a Failed
    /// (cable glitch, dongle yank, bad first try) the Reconnect button restores them 1:1.
    fn save_last_connection(&mut self) {
        let cp = &self.control_panel;
        let serial_port_name = cp
            .selected_serial_port
            .and_then(|i| cp.serial_ports.get(i))
            .map(|p| p.port_name.clone());

        let channels = match cp.selected_source {
            DataSourceType::Synthetic => cp.synthetic_channels,
            DataSourceType::CytonSerial | DataSourceType::CytonWifi => cp.cyton_channels,
            DataSourceType::GanglionNative => 4,
            _ => 8,
        };

        self.last_connection = Some(LastConnectionParams {
            source: cp.selected_source,
            serial_port_name,
            channels,
            playback_file: cp.playback_file.clone(),
        });

        self.event_log.log_system(&format!(
            "Saved last connection settings for quick Reconnect: {:?}",
            self.last_connection.as_ref().map(|p| &p.source)
        ));
    }

    // === Phase 8 Persistence helpers (smallest effective impl, debug-only on failure) ===

    fn config_path() -> Option<std::path::PathBuf> {
        // Phase 8: platform config locations (macOS ~/Library/Application Support/gui-rust/, Linux ~/.config/openbci-gui-rust/, Windows %APPDATA%\gui-rust\)
        let proj = ProjectDirs::from("com", "openbci", "gui-rust")?;
        let dir = proj.config_dir();
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::debug!("Could not create config dir {:?}: {}", dir, e);
            return None;
        }
        Some(dir.join("config.json"))
    }

    fn load_persisted_settings() -> PersistedSettings {
        if let Some(p) = Self::config_path() {
            match std::fs::read_to_string(&p) {
                Ok(data) => match serde_json::from_str::<PersistedSettings>(&data) {
                    Ok(s) => {
                        tracing::debug!("Loaded persisted settings from {:?}", p);
                        return s;
                    }
                    Err(e) => tracing::debug!(
                        "Corrupt/incompatible config.json at {:?} ({}), using defaults",
                        p,
                        e
                    ),
                },
                Err(e) => tracing::debug!("No config or read error at {:?} ({}), defaults", p, e),
            }
        }
        PersistedSettings::default()
    }

    fn save_persisted_settings(s: &PersistedSettings) {
        if let Some(p) = Self::config_path() {
            match serde_json::to_string_pretty(s) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&p, json) {
                        tracing::debug!("Failed to write persisted settings to {:?}: {}", p, e);
                    } else {
                        tracing::debug!("Saved persisted settings to {:?}", p);
                    }
                }
                Err(e) => tracing::debug!("Failed to serialize persisted settings: {}", e),
            }
        }
    }

    fn current_persisted_settings(&self) -> PersistedSettings {
        let cp = &self.control_panel;
        let last_serial_port = cp
            .selected_serial_port
            .and_then(|i| cp.serial_ports.get(i))
            .map(|p| p.port_name.clone());

        let mut udp_enabled = false;
        let mut udp_target = "127.0.0.1:12345".to_string();
        let mut osc_enabled = false;
        let mut osc_target = "127.0.0.1:9000".to_string();
        let mut lsl_enabled = false;
        let mut lsl_target = crate::networking::lsl_stream::DEFAULT_EEG_NAME.to_string();
        for c in &self.networking.configs {
            match c.protocol {
                Protocol::UDP => {
                    udp_enabled = c.enabled;
                    if !c.target.is_empty() {
                        udp_target = c.target.clone();
                    }
                }
                Protocol::OSC => {
                    osc_enabled = c.enabled;
                    if !c.target.is_empty() {
                        osc_target = c.target.clone();
                    }
                }
                Protocol::LSL => {
                    lsl_enabled = c.enabled;
                    if !c.target.is_empty() {
                        lsl_target = c.target.clone();
                    }
                }
            }
        }
        let ads_channels = self
            .board
            .as_ref()
            .and_then(|b| b.ads_channels().map(|s| s.to_vec()))
            .unwrap_or_else(|| self.persisted_ads_channels.clone());

        // Snapshot current graph speed/stability settings from live widgets (Post-Phase 8 wave)
        let (ts_tw, ts_ys) = self
            .widget_manager
            .widgets
            .iter()
            .find_map(|w| w.as_any().downcast_ref::<WTimeSeries>())
            .map(|ts| (ts.time_window_sec(), ts.y_scale_uv()))
            .unwrap_or((
                self.persisted_ts_time_window_sec,
                self.persisted_ts_y_scale_uv,
            ));

        let fft_si = self
            .widget_manager
            .widgets
            .iter()
            .find_map(|w| w.as_any().downcast_ref::<WFFT>())
            .map(|f| f.smoothing_index())
            .unwrap_or(self.persisted_fft_smoothing_index);

        let bp_si = self
            .widget_manager
            .widgets
            .iter()
            .find_map(|w| w.as_any().downcast_ref::<WBandPower>())
            .map(|b| b.smoothing_index())
            .unwrap_or(self.persisted_bp_smoothing_index);

        let ts_per_ch = self
            .widget_manager
            .widgets
            .iter()
            .find_map(|w| w.as_any().downcast_ref::<WTimeSeries>())
            .map(|ts| ts.per_channel_y_scales())
            .unwrap_or_else(|| self.persisted_ts_per_channel_y_scales.clone());

        PersistedSettings {
            selected_source: cp.selected_source,
            synthetic_channels: cp.synthetic_channels,
            cyton_channels: cp.cyton_channels,
            last_serial_port,
            playback_file: cp.playback_file.clone(),
            recording_format: self.recording_format,
            filter_notch_enabled: self.last_persisted_notch_mode != NotchMode::Off,
            filter_notch_mode: Some(self.last_persisted_notch_mode),
            filter_bandpass_enabled: self.last_persisted_filter_bandpass,
            filter_bandpass_low: self.last_persisted_filter_bandpass_low,
            filter_bandpass_high: self.last_persisted_filter_bandpass_high,
            udp_enabled,
            udp_target,
            osc_enabled,
            osc_target,
            lsl_enabled,
            lsl_target,
            ads_channels,
            cyton_wifi_ip: cp.cyton_wifi_ip.clone(),
            ganglion_device_id: cp.ganglion_device_id.clone(),
            sd_file: cp.sd_file.clone(),

            ts_time_window_sec: ts_tw,
            ts_y_scale_uv: ts_ys,
            fft_smoothing_index: fft_si,
            bp_smoothing_index: bp_si,
            ts_per_channel_y_scales: ts_per_ch,
            current_layout: self.current_layout,
            font_sizes: self.font_sizes.clone(),
            head_show_waves: self.head_show_waves,
            head_show_hemispheres: self.head_show_hemispheres,
        }
    }

    fn switch_synthetic_channels(&mut self, n: usize) {
        if let Some(mut old) = self.board.take() {
            let _ = old.stop_streaming();
            let _ = old.uninitialize();
        }
        let mut board = BrainFlowBoard::synthetic(n);
        let _ = board.initialize();
        match board.start_streaming() {
            Ok(()) => self.streaming = true,
            Err(e) => {
                self.streaming = false;
                tracing::error!("Synthetic {}ch start failed: {}", n, e);
            }
        }
        self.board = Some(Box::new(board) as Box<dyn DataSource>);
        self.control_panel.synthetic_channels = n;
        self.populate_widgets_for_new_session();
        self.populate_tool_widgets();
        self.apply_persisted_filters_to_current_board();
        self.save_current_persisted_settings();
        self.event_log
            .log_system(&format!("Switched Synthetic board to {} channels", n));
    }

    fn save_current_persisted_settings(&self) {
        let s = self.current_persisted_settings();
        Self::save_persisted_settings(&s);
    }

    fn apply_persisted_filters_to_current_board(&mut self) {
        let n = self
            .board
            .as_ref()
            .and_then(|b| b.get_filter_settings().map(|s| s.channels.len()))
            .or_else(|| self.board.as_ref().map(|b| b.exg_channels().len()))
            .unwrap_or(0);
        if let Some(ref mut board) = self.board {
            let (enabled, noise) = self.last_persisted_notch_mode.to_brainflow();
            let sr = board.sample_rate() as f64;
            let (lo, hi) = crate::filter_settings::applied_bandpass_corners(
                self.last_persisted_filter_bandpass_low,
                self.last_persisted_filter_bandpass_high,
                sr,
            );
            for ch in 0..n {
                board.set_notch_filter(ch, enabled, noise);
                board.set_bandpass_filter(ch, self.last_persisted_filter_bandpass, lo, hi);
            }
            board.apply_pending_filters();
        }
        self.apply_persisted_ads_to_current_board();
        if let Some(ref b) = self.board {
            self.networking
                .set_lsl_geometry(b.exg_channels().len(), b.sample_rate() as f64);
        }
    }

    fn apply_persisted_ads_to_current_board(&mut self) {
        if self.persisted_ads_channels.is_empty() {
            return;
        }
        let n = self
            .board
            .as_ref()
            .and_then(|b| b.ads_channels().map(|s| s.len()))
            .unwrap_or(0);
        if n == 0 {
            return;
        }
        let bank: Vec<AdsChannel> = self
            .persisted_ads_channels
            .iter()
            .cloned()
            .chain(default_bank(n))
            .take(n)
            .collect();
        if let Some(ref mut board) = self.board {
            for (i, s) in bank.into_iter().enumerate() {
                if s != AdsChannel::default() {
                    if let Err(e) = board.commit_ads_channel(i, s) {
                        tracing::warn!("ADS commit ch{} failed: {e}", i + 1);
                    }
                }
            }
        }
    }

    fn draw_layout_slots(&mut self, ui: &mut egui::Ui) {
        let count = WidgetManager::container_count_for(self.current_layout);
        let options = [
            "Time Series",
            "FFT Plot",
            "Band Power",
            "Accelerometer",
            "Head Plot",
            "Left / right",
            "Which first",
            "Impedance",
            "Spectrogram",
            "EMG",
            "EMG Joystick",
            "Analog Read",
            "Digital Read",
            "Pulse Sensor",
            "Board",
        ];
        let mut pending = false;
        let layout_id = self.current_layout;
        {
            let assignment = self
                .grid_layout_assignments
                .entry(layout_id)
                .or_insert_with(|| vec!["Time Series".into(); count]);
            while assignment.len() < count {
                assignment.push("Time Series".into());
            }
            assignment.truncate(count);
            for i in 0..assignment.len() {
                let chosen = ui
                    .horizontal(|ui| {
                        let mut current = assignment[i].clone();
                        ui.small(format!("{}", i + 1));
                        egui::ComboBox::from_id_salt(format!("layout_slot_{layout_id}_{i}"))
                            .selected_text(current.clone())
                            .show_ui(ui, |ui| {
                                for &opt in &options {
                                    ui.selectable_value(&mut current, opt.to_string(), opt);
                                }
                            });
                        current
                    })
                    .inner;
                if chosen != assignment[i] {
                    assignment[i] = chosen;
                    pending = true;
                }
            }
        }
        if pending {
            self.pending_layout_rebuild = true;
        }
    }

    fn draw_record_export(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if !self.data_logger.is_logging() {
                egui::ComboBox::from_id_salt("rec_fmt")
                    .selected_text(format!("{:?}", self.recording_format))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.recording_format,
                            crate::data_logger::LogFormat::BDF,
                            "BDF",
                        );
                        ui.selectable_value(
                            &mut self.recording_format,
                            crate::data_logger::LogFormat::ODF,
                            "ODF",
                        );
                    });
            }
            let record_label = if self.data_logger.is_logging() {
                "Stop Rec"
            } else {
                "Record"
            };
            let live_hw = self.board.as_ref().is_some_and(|b| {
                let n = b.name();
                n != "Playback" && !n.contains("Synthetic")
            });
            let record_color = if self.data_logger.is_logging() {
                theme::STOP
            } else if live_hw {
                theme::START
            } else {
                theme::PANEL
            };
            let mut record_btn = egui::Button::new(record_label).fill(record_color);
            if !live_hw && !self.data_logger.is_logging() {
                record_btn = record_btn.stroke(theme::hairline());
            }
            if ui.add(record_btn).clicked() {
                if self.data_logger.is_logging() {
                    self.stop_recording_like_session();
                } else {
                    let _ = self.start_recording_like_session();
                }
            }
            if !self.data_logger.is_logging() && ui.button("Export").clicked() {
                let path = self.last_recording_path.clone().or_else(|| {
                    self.control_panel
                        .playback_file
                        .as_ref()
                        .map(std::path::PathBuf::from)
                });
                match path {
                    Some(p) => match crate::board::playback::PlaybackBoard::from_file(&p) {
                        Ok(pb) => {
                            match crate::export::export_next_to(
                                &p,
                                pb.export_samples(),
                                pb.sample_rate(),
                                pb.exg_channels().len(),
                                pb.session_markers(),
                            ) {
                                Ok((csv, jsonl)) => {
                                    self.connection_status = format!("Exported {}", csv.display());
                                    self.event_log.log_recording(&format!(
                                        "Feature export → {} / {}",
                                        csv.display(),
                                        jsonl.display()
                                    ));
                                }
                                Err(e) => {
                                    self.event_log.log_error(&format!("Export failed: {e}"));
                                }
                            }
                        }
                        Err(e) => {
                            self.event_log
                                .log_error(&format!("Export: cannot open recording: {e}"));
                        }
                    },
                    None => {
                        if let Some(picked) = rfd::FileDialog::new()
                            .set_title("Export recording")
                            .add_filter("Recordings", &["bdf", "odf", "txt", "csv"])
                            .pick_file()
                        {
                            self.last_recording_path = Some(picked.clone());
                            self.control_panel.playback_file = Some(picked.display().to_string());
                            self.connection_status = format!("Export: chose {}", picked.display());
                        } else {
                            self.connection_status =
                                "Export: record a session or choose a file".into();
                            self.event_log.log_error(
                                "Export: record a session or pick a Playback file first",
                            );
                        }
                    }
                }
            }
        });
    }

    fn draw_session_rack(&mut self, ui: &mut egui::Ui, exclusive_open: &mut Option<String>) {
        let session_is_open = exclusive_open.is_none();
        let resp = egui::CollapsingHeader::new("Session")
            .open(Some(session_is_open))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Layout");
                    egui::ComboBox::from_id_salt("layout_select")
                        .selected_text(match self.current_layout {
                            1 => "1 Full",
                            2 => "2 Quad",
                            3 => "3 Split V",
                            4 => "4 Split H",
                            5 => "5 Tall L",
                            6 => "6 Tall R",
                            _ => "Layout",
                        })
                        .show_ui(ui, |ui| {
                            let opts = [
                                (1, "1  Full"),
                                (2, "2  Quad"),
                                (3, "3  Split vertical"),
                                (4, "4  Split horizontal"),
                                (5, "5  Tall left"),
                                (6, "6  Tall right"),
                            ];
                            for (num, label) in opts {
                                if ui
                                    .selectable_value(&mut self.current_layout, num, label)
                                    .changed()
                                {
                                    self.set_layout(num);
                                    self.event_log
                                        .log_system(&format!("Layout changed to {}", label));
                                }
                            }
                        });
                });
                self.draw_layout_slots(ui);

                let mut persist_filters = false;
                ui.horizontal(|ui| {
                    ui.label("Notch");
                    let mut mode = self.last_persisted_notch_mode;
                    let mut notch_changed = false;
                    egui::ComboBox::from_id_salt("notch_mode")
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for m in NotchMode::ALL {
                                if ui.selectable_value(&mut mode, m, m.label()).changed() {
                                    notch_changed = true;
                                }
                            }
                        });
                    if notch_changed {
                        self.last_persisted_notch_mode = mode;
                        persist_filters = true;
                        self.event_log
                            .log_filter(&format!("Notch set to {}", mode.label()));
                    }
                    let mut bp = self.last_persisted_filter_bandpass;
                    if ui.checkbox(&mut bp, "BP").changed() {
                        self.last_persisted_filter_bandpass = bp;
                        persist_filters = true;
                    }
                    let mut lo = self.last_persisted_filter_bandpass_low;
                    if ui
                        .add(
                            egui::DragValue::new(&mut lo)
                                .range(0.1..=500.0)
                                .speed(0.5)
                                .max_decimals(1)
                                .prefix("low ")
                                .suffix(" Hz"),
                        )
                        .changed()
                    {
                        self.last_persisted_filter_bandpass_low = lo;
                        persist_filters = true;
                    }
                    let mut hi = self.last_persisted_filter_bandpass_high;
                    if ui
                        .add(
                            egui::DragValue::new(&mut hi)
                                .range(0.1..=500.0)
                                .speed(0.5)
                                .max_decimals(1)
                                .prefix("high ")
                                .suffix(" Hz"),
                        )
                        .changed()
                    {
                        self.last_persisted_filter_bandpass_high = hi;
                        persist_filters = true;
                    }
                });
                ui.label("Smooth cutoff");
                let sr = self
                    .board
                    .as_ref()
                    .map(|b| b.sample_rate() as f64)
                    .unwrap_or(250.0);
                ui.label(crate::filter_settings::nyquist_readout(sr));
                if persist_filters {
                    self.apply_persisted_filters_to_current_board();
                    self.save_current_persisted_settings();
                }

                if self
                    .board
                    .as_ref()
                    .is_some_and(|b| b.name().contains("Synthetic"))
                {
                    ui.horizontal(|ui| {
                        if ui.button("8 ch").clicked() {
                            self.switch_synthetic_channels(8);
                        }
                        if ui.button("16 ch").clicked() {
                            self.switch_synthetic_channels(16);
                        }
                    });
                }
            });
        if resp.header_response.clicked() && !session_is_open {
            *exclusive_open = None;
        }
    }
}

impl eframe::App for OpenBciGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count += 1;
        if let Ok(crop) = std::env::var("OPENBCI_CROP") {
            if self.frame_count == 90 || self.frame_count == 140 || self.frame_count == 220 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Screenshot { image, .. } = ev {
                        let [w, h] = image.size;
                        let mut buf = format!("P6\n{w} {h}\n255\n").into_bytes();
                        buf.reserve(w * h * 3);
                        for px in &image.pixels {
                            let a = px.to_array();
                            buf.extend_from_slice(&[a[0], a[1], a[2]]);
                        }
                        let _ = std::fs::write(&crop, buf);
                    }
                }
            });
        }
        theme::apply_visuals(ctx);
        self.font_sizes.apply_egui(ctx);
        theme::set_font_sizes(self.font_sizes.clone());

        // === Handle background connection to real hardware ===
        if let ConnectionState::InProgress {
            receiver,
            status_message,
        } = &mut self.connection_state
        {
            match receiver.try_recv() {
                Ok(Ok(mut connected_board)) => {
                    self.connection_status = format!("Connected to {}", connected_board.name());
                    // Auto-start streaming so the user sees real data immediately
                    if let Err(e) = connected_board.start_streaming() {
                        tracing::error!(
                            "Failed to auto-start streaming after hardware connection: {}",
                            e
                        );
                    } else {
                        self.streaming = true;
                    }
                    self.board = Some(Box::new(connected_board) as Box<dyn DataSource>); // plan.md Phase 7 — polymorphic board (Playback + Live)
                    self.event_log
                        .log_connection(&format!("Connected to {}", self.connection_status));
                    // Phase 7 Reconnect: remember this successful real-hardware connect
                    self.save_last_connection();
                    self.enter_running_session();
                    self.connection_state = ConnectionState::Idle;
                }
                Ok(Err(err)) => {
                    // Give the user actionable guidance, especially for the very common macOS serial port problem
                    let friendly = if err.contains("BrainFlow")
                        || err.contains("serial")
                        || err.contains("port")
                    {
                        #[cfg(target_os = "macos")]
                        {
                            format!(
                                "Failed to open Cyton on {}.\n\n\
                                 Common macOS fix: Use a port that starts with 'cu.usbserial' instead of 'tty.usbserial'.\n\
                                 Unplug the dongle, plug it back in, then Refresh the port list.\n\n\
                                 Technical error: {}",
                                // We don't have the port easily here, but the user just saw it in the UI
                                "the selected port", err
                            )
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            format!("Failed to connect to Cyton: {}. Check cable, permissions, and that no other program is using the port.", err)
                        }
                    } else {
                        format!("Connection failed: {}", err)
                    };

                    self.connection_status = friendly;
                    tracing::error!("Hardware connection failed: {}", err);
                    self.event_log
                        .log_error(&format!("Connection failed: {}", err));
                    self.connection_state = ConnectionState::Failed(err.clone());
                    // Stay in PreInit so user can try again or choose Synthetic
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    // Still connecting — show a nice connecting screen
                    let msg = status_message.clone();
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(100.0);
                            ui.heading("Connecting...");
                            ui.label(msg);
                            ui.add_space(20.0);
                            ui.spinner();
                            ui.add_space(30.0);
                            if ui.button("Cancel").clicked() {
                                self.connection_state = ConnectionState::Idle;
                                self.connection_status = "Connection cancelled".to_string();
                            }
                        });
                    });
                    return; // Don't draw the normal control panel while connecting
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.connection_status = "Connection channel closed unexpectedly".to_string();
                    self.connection_state = ConnectionState::Idle;
                }
            }
        }

        if setup_panel_active(self.system_mode) {
            // === Control Panel (PreInit) ===
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some((source, chans, serial_port)) = self.control_panel.draw(ui) {
                    // User clicked "Start Session" — clear any previous Failed state (plan.md Phase 7 Reconnect)
                    if matches!(self.connection_state, ConnectionState::Failed(_)) {
                        self.connection_state = ConnectionState::Idle;
                        self.connection_status.clear();
                    }
                    // User clicked "Start Session"
                    match source {
                        DataSourceType::Synthetic => {
                            self.connection_status = "Using BrainFlow Synthetic Board".to_string();
                            let mut board = BrainFlowBoard::synthetic(chans);
                            let _ = board.initialize();
                            // Auto-start streaming so the user immediately sees graphs
                            if let Err(e) = board.start_streaming() {
                                tracing::error!("Failed to auto-start streaming on Synthetic: {}", e);
                            } else {
                                self.streaming = true;
                            }
                            self.board = Some(Box::new(board) as Box<dyn DataSource>);
                            self.event_log.log_connection("Connected to BrainFlow Synthetic board");
                            // Phase 7 Reconnect: remember the Synthetic settings (chans etc.)
                            self.save_last_connection();
                            self.enter_running_session();
                        }
                        DataSourceType::CytonSerial => {
                            // Prefer cu.* on macOS by default (tty.* versions are a very common source of failure)
                            let default_port = if cfg!(target_os = "macos") {
                                "/dev/cu.usbserial-0000".to_string()
                            } else {
                                "/dev/tty.usbserial-0000".to_string()
                            };
                            let port = serial_port.clone().unwrap_or(default_port);
                            let is_daisy = chans >= 16;
                            tracing::info!("Starting background connection to Cyton{} on port: {}", if is_daisy { " + Daisy" } else { "" }, port);

                            let port_for_thread = port.clone();
                            let (tx, rx) = oneshot::channel();

                            // Spawn the blocking connection on a separate thread so we don't freeze the UI
                            std::thread::spawn(move || {
                                let mut board = if is_daisy {
                                    BrainFlowBoard::cyton_serial_daisy(&port_for_thread)
                                } else {
                                    BrainFlowBoard::cyton_serial(&port_for_thread)
                                };
                                let result = board.initialize().map(|_| board).map_err(|e| e.to_string());
                                let _ = tx.send(result);
                            });

                            self.connection_state = ConnectionState::InProgress {
                                receiver: rx,
                                status_message: format!("Connecting to Cyton on {}...", port),
                            };
                            // Stay in PreInit visually until the connection finishes
                        }
                        DataSourceType::CytonWifi => {
                            let ip = serial_port.clone().unwrap_or_default();
                            if ip.trim().is_empty() {
                                self.control_panel.show = true;
                                self.control_panel.last_setup_error =
                                    Some("Enter the WiFi shield IP address.".into());
                                return;
                            }
                            let daisy = chans >= 16;
                            let ip_for_thread = ip.clone();
                            let (tx, rx) = oneshot::channel();
                            std::thread::spawn(move || {
                                let mut board =
                                    BrainFlowBoard::cyton_wifi(&ip_for_thread, daisy);
                                let result =
                                    board.initialize().map(|_| board).map_err(|e| e.to_string());
                                let _ = tx.send(result);
                            });
                            self.connection_state = ConnectionState::InProgress {
                                receiver: rx,
                                status_message: format!("Connecting to Cyton WiFi {}...", ip),
                            };
                        }
                        DataSourceType::GanglionNative => {
                            let id = serial_port.clone().unwrap_or_default();
                            if id.trim().is_empty() {
                                self.connection_status =
                                    "Ganglion requires a MAC / device name".to_string();
                                self.control_panel.show = true;
                                self.control_panel.last_setup_error = Some(
                                    "Ganglion: enter a MAC / device name. This is not Synthetic."
                                        .into(),
                                );
                                return;
                            }
                            tracing::info!("Starting background connection to Ganglion Native {}", id);
                            let id_for_thread = id.clone();
                            let (tx, rx) = oneshot::channel();
                            std::thread::spawn(move || {
                                let mut board = BrainFlowBoard::ganglion_native(&id_for_thread);
                                let result =
                                    board.initialize().map(|_| board).map_err(|e| e.to_string());
                                let _ = tx.send(result);
                            });
                            self.connection_state = ConnectionState::InProgress {
                                receiver: rx,
                                status_message: format!("Connecting to Ganglion {}...", id),
                            };
                        }
                        DataSourceType::Playback => {
                            // Phase 7 killer feature: real Playback roundtrip (plan.md Phase 7 step 3)
                            // Record (ODF .txt) → End Session → pick exact same file → all widgets (incl. Focus audio, markers, Networking, Console) replay the session.
                            let file_path = serial_port.clone().unwrap_or_default();
                            if file_path.is_empty() {
                                self.connection_status = "No playback file selected".to_string();
                                self.event_log.log_error("Playback started with no file");
                                return;
                            }
                            self.connection_status = format!("Loading playback: {}", file_path);

                            match crate::board::playback::PlaybackBoard::from_file(std::path::Path::new(&file_path)) {
                                Ok(mut pb) => {
                                    let _ = pb.initialize();
                                    if let Err(e) = pb.start_streaming() {
                                        tracing::error!("Playback start failed: {}", e);
                                        self.event_log.log_error(&format!("Playback failed to start: {}", e));
                                    } else {
                                        self.streaming = true;
                                    }
                                    // Now the board is the real PlaybackBoard — widgets will receive replayed EEG frames.
                                    self.board = Some(Box::new(pb) as Box<dyn DataSource>);
                                    self.connection_status = format!("Playback: {}", file_path);
                                    self.event_log.log_connection(&format!(
                                        "Playback · {}",
                                        std::path::Path::new(&file_path)
                                            .file_name()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("file"),
                                    ));
                                    // Phase 7 Reconnect: remember the exact playback file so "Reconnect" replays the same recording
                                    self.save_last_connection();
                                    self.enter_running_session();
                                }
                                Err(e) => {
                                    self.connection_status = format!("Failed to load playback file: {}", e);
                                    self.event_log.log_error(&format!("Playback file load error: {}", e));
                                }
                            }
                        }
                        DataSourceType::SDCard => {
                            let file_path = serial_port.clone().unwrap_or_default();
                            if file_path.is_empty() {
                                self.control_panel.show = true;
                                self.control_panel.last_setup_error =
                                    Some("Choose a Cyton SD hex file first.".into());
                                return;
                            }
                            match crate::board::playback::PlaybackBoard::from_sd(std::path::Path::new(&file_path))
                            {
                                Ok(mut pb) => {
                                    let _ = pb.initialize();
                                    if pb.start_streaming().is_ok() {
                                        self.streaming = true;
                                    }
                                    self.board = Some(Box::new(pb) as Box<dyn DataSource>);
                                    self.connection_status = format!("SD playback: {}", file_path);
                                    self.event_log.log_connection(&format!("SD card playback {file_path}"));
                                    self.save_last_connection();
                                    self.enter_running_session();
                                }
                                Err(e) => {
                                    self.control_panel.show = true;
                                    self.control_panel.last_setup_error = Some(format!("{e}"));
                                    self.event_log.log_error(&format!("SD file: {e}"));
                                }
                            }
                        }
                    }
                }

                // Phase 7 Reconnect banner
                // The button restores the last good params (port, chans, file) into the control panel dropdowns
                // and for instant sources (Synthetic/Playback) performs a true 1-click reconnect. Cyton gets
                // the dropdowns fixed + a hint to hit the normal Start (the background thread path is not duped).
                // We extract the needed data *before* the egui closure to satisfy the borrow checker.
                let reconnect_info: Option<(String, Option<LastConnectionParams>)> =
                    if let ConnectionState::Failed(ref m) = self.connection_state {
                        Some((m.clone(), self.last_connection.clone()))
                    } else {
                        None
                    };
                if let Some((err_msg, last_params)) = reconnect_info {
                    ui.add_space(10.0);
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(55, 25, 25))
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(255, 180, 180), "⚠️  Last connection attempt failed");
                                ui.small(&err_msg);
                                ui.add_space(6.0);
                                if let Some(ref params) = last_params {
                                    let label = match params.source {
                                        DataSourceType::CytonSerial => format!("Cyton{} on {}", if params.channels >= 16 { " + Daisy" } else { "" }, params.serial_port_name.as_deref().unwrap_or("selected port")),
                                        DataSourceType::CytonWifi => format!("Cyton WiFi {}", self.control_panel.cyton_wifi_ip),
                                        DataSourceType::Synthetic => format!("Synthetic ({} ch)", params.channels),
                                        DataSourceType::Playback => "the same Playback file".to_string(),
                                        DataSourceType::SDCard => "the same SD file".to_string(),
                                        DataSourceType::GanglionNative => {
                                            format!("Ganglion {}", self.control_panel.ganglion_device_id)
                                        }
                                    };
                                    if ui.button(egui::RichText::new(format!("🔄 Reconnect using {}", label)).strong()).clicked() {
                                        // Restore exact previous choices into the visible control panel
                                        self.control_panel.selected_source = params.source;
                                        if let Some(ref name) = params.serial_port_name {
                                            if let Some(idx) = self.control_panel.serial_ports.iter().position(|p| &p.port_name == name) {
                                                self.control_panel.selected_serial_port = Some(idx);
                                            }
                                        }
                                        match params.source {
                                            DataSourceType::Synthetic => self.control_panel.synthetic_channels = params.channels,
                                            DataSourceType::CytonSerial | DataSourceType::CytonWifi => self.control_panel.cyton_channels = params.channels,
                                            _ => {}
                                        }
                                        self.control_panel.playback_file = params.playback_file.clone();

                                        self.connection_status = "Reconnect settings restored".into();
                                        self.event_log.log_system(&format!("Reconnect used — restored {:?}", params.source));

                                        // True 1-click for the paths that don't need background thread
                                        if params.source == DataSourceType::Synthetic {
                                            self.connection_status = "Using BrainFlow Synthetic Board (Reconnect)".to_string();
                                            let mut board = BrainFlowBoard::synthetic(params.channels);
                                            let _ = board.initialize();
                                            if let Err(e) = board.start_streaming() { tracing::error!("Reconnect synth: {}", e);} else { self.streaming = true; }
                                            self.board = Some(Box::new(board) as Box<dyn DataSource>);
                                            self.event_log.log_connection("Connected to Synthetic (Reconnect)");
                                            self.save_last_connection();
                                            self.enter_running_session();
                                            self.connection_state = ConnectionState::Idle;
                                        } else if params.source == DataSourceType::Playback {
                                            if let Some(ref f) = params.playback_file {
                                                if let Ok(mut pb) = crate::board::playback::PlaybackBoard::from_file(std::path::Path::new(f)) {
                                                    let _ = pb.initialize();
                                                    if let Err(e) = pb.start_streaming() { tracing::error!("Reconnect pb: {}", e);} else { self.streaming = true; }
                                                    self.board = Some(Box::new(pb) as Box<dyn DataSource>);
                                                    self.connection_status = format!("Playback: {} (Reconnect)", f);
                                                    self.event_log.log_connection(&format!(
                                                        "Playback · {}",
                                                        std::path::Path::new(f)
                                                            .file_name()
                                                            .and_then(|s| s.to_str())
                                                            .unwrap_or("file"),
                                                    ));
                                                    self.save_last_connection();
                                                    self.enter_running_session();
                                                    self.connection_state = ConnectionState::Idle;
                                                }
                                            }
                                        } else {
                                            // Cyton or other: dropdowns are now correct; user clicks the normal Start Session (green) button.
                                            self.connection_status = "Panel restored — click the big Start Session button to retry".to_string();
                                        }
                                    }
                                } else {
                                    ui.small("No previous successful connection to replay yet.");
                                }
                            });
                        });
                }
            });
            // Session may have started this frame (Synthetic / Playback / SD). Fall through to transport.
            if setup_panel_active(self.system_mode) {
                ctx.request_repaint();
                return;
            }
        }

        // === PostInit (normal GUI) ===
        if self.streaming {
            // Phase 7: as_deref_mut() gives Option<&mut dyn DataSource> (works for BrainFlowBoard and PlaybackBoard)
            if let Some(b) = self.board.as_deref_mut() {
                b.update();

                // Phase 7 WPacketLoss (plan.md Phase 7 polish): accurate tracking using the new
                // DataSource::recent_samples_delivered() hook. We accumulate the actual delivered
                // count over the wall-time window (instead of the old buggy get_data(1).len() == ~1).
                // Result: the SidePanel sparkline + % is meaningful, and Playback roundtrips report
                // essentially 0% because advance exactly matches elapsed * sr * speed.
                let delivered_this_tick = b.recent_samples_delivered() as u64;
                let lost_this_tick = b.recent_samples_lost() as u64;
                if delivered_this_tick > 0 {
                    self.samples_received += delivered_this_tick;
                    self.window_samples += delivered_this_tick;
                    self.window_lost += lost_this_tick;
                }

                let now = std::time::Instant::now();
                if let Some(last) = self.last_sample_time {
                    let elapsed = now.duration_since(last).as_secs_f64();
                    if elapsed > 1.0 {
                        let received_in_window = self.window_samples;
                        let lost_in_window = self.window_lost;
                        // Java PacketLossTracker: gaps in sample index, not wall-clock vs 250 Hz.
                        // A slow UI frame used to look like 5–20% loss.
                        let instant_loss =
                            crate::stream_stats::loss_percent(received_in_window, lost_in_window);
                        self.packet_loss_percent = crate::stream_stats::smooth(
                            self.packet_loss_percent,
                            instant_loss,
                            0.35,
                        );
                        if received_in_window > 0 {
                            let instant_hz = received_in_window as f64 / elapsed;
                            self.current_sample_rate = if self.current_sample_rate == 0.0 {
                                instant_hz
                            } else {
                                crate::stream_stats::smooth(
                                    self.current_sample_rate,
                                    instant_hz,
                                    0.35,
                                )
                            };
                        }
                        self.last_sample_time = Some(now);
                        self.window_samples = 0;
                        self.window_lost = 0;
                        self.packet_loss_history
                            .push(self.packet_loss_percent as f32);
                        if self.packet_loss_history.len() > 60 {
                            self.packet_loss_history.remove(0);
                        }
                    }
                } else {
                    self.last_sample_time = Some(now);
                    self.window_samples = 0;
                    self.window_lost = 0;
                }

                // Recording: raw EXG (Java ODF/BDF), never the display-filtered buffer.
                if self.data_logger.is_logging() {
                    let exg = b.exg_channels().to_vec();
                    let latest = recent_raw_rows(b);
                    for row in latest {
                        let eeg = extract_exg(&row, &exg);
                        self.data_logger.log_sample(&eeg, 0.0);
                    }
                }

                self.networking.push_data(b);
            }
        }

        if let Some(b) = self.board.as_deref() {
            self.emg.process_source(b);
        }

        // Phase 7 hybrid + Issue 3 fix: tool widgets (HeadPlot, Impedance, Focus, Marker, Networking)
        // must keep receiving data and servicing actions *even when streaming is paused*.
        // The heavy acquisition / packet-loss / recording / networking-push stay inside the streaming guard above.
        let mut persist_ads = false;
        if let Some(b) = self.board.as_deref_mut() {
            for t in &mut self.tool_widgets {
                t.update(b);
            }

            // Impedance polling (WImpedance buttons request via pending flags)
            for t in &mut self.tool_widgets {
                if let Some(imp) = t.as_any_mut().downcast_mut::<WImpedance>() {
                    if imp.wants_start() {
                        let chs: Vec<usize> = (0..b.exg_channels().len()).collect();
                        match b.start_impedance_test(&chs) {
                            Ok(()) => {
                                imp.clear_start_error();
                                self.event_log.log_system("Impedance test started on board");
                            }
                            Err(e) => {
                                let msg = format!("Impedance start failed: {e}");
                                imp.set_start_error(&msg);
                                self.event_log.log_error(&msg);
                            }
                        }
                        imp.clear_pending();
                    }
                    if imp.wants_stop() {
                        match b.stop_impedance_test() {
                            Ok(()) => {
                                self.event_log.log_system("Impedance test stopped");
                            }
                            Err(e) => {
                                let msg = format!("Impedance stop failed: {e}");
                                imp.set_start_error(&msg);
                                self.event_log.log_error(&msg);
                            }
                        }
                        imp.clear_pending();
                    }
                    if let Some(e) = b.take_impedance_error() {
                        let msg = format!("Impedance scan aborted: {e}");
                        imp.set_start_error(&msg);
                        self.event_log.log_error(&msg);
                    }
                }
                if let Some(hw) = t.as_any_mut().downcast_mut::<WHardwareSettings>() {
                    if let Some((ch, settings)) = hw.take_pending() {
                        match b.commit_ads_channel(ch, settings) {
                            Ok(()) => {
                                self.persisted_ads_channels =
                                    b.ads_channels().map(|s| s.to_vec()).unwrap_or_default();
                                persist_ads = true;
                                self.event_log.log_system(&format!(
                                    "Board ch{} → {:?}",
                                    ch + 1,
                                    settings.power
                                ));
                            }
                            Err(e) => {
                                self.event_log.log_error(&format!("Board failed: {e}"));
                            }
                        }
                    }
                }
                if let Some(w) = t.as_any_mut().downcast_mut::<WAnalogRead>() {
                    if let Some(mode) = w.take_pending_mode() {
                        if let Err(e) = b.set_cyton_board_mode(mode) {
                            self.event_log.log_error(&format!("Analog mode: {e}"));
                        }
                    }
                }
                if let Some(w) = t.as_any_mut().downcast_mut::<WDigitalRead>() {
                    if let Some(mode) = w.take_pending_mode() {
                        if let Err(e) = b.set_cyton_board_mode(mode) {
                            self.event_log.log_error(&format!("Digital mode: {e}"));
                        }
                    }
                }
                if let Some(w) = t.as_any_mut().downcast_mut::<WPulseSensor>() {
                    if let Some(mode) = w.take_pending_mode() {
                        if let Err(e) = b.set_cyton_board_mode(mode) {
                            self.event_log.log_error(&format!("Pulse analog mode: {e}"));
                        }
                    }
                }
            }
        }
        if persist_ads {
            self.save_current_persisted_settings();
        }

        self.tick_contact_sidecar();

        if self.experiment.is_running() {
            self.tick_experiment();
            ctx.request_repaint();
        }

        // Thin transport (Ableton / Resolve), not a 64px Java navy header.
        egui::TopBottomPanel::top("top_nav")
            .exact_height(32.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::TRANSPORT)
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let stream_label = if self.streaming { "Stop" } else { "Start" };
                    let stream_fill = if self.streaming {
                        theme::STOP
                    } else {
                        theme::START
                    };
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(stream_label).color(theme::TEXT))
                                .fill(stream_fill)
                                .min_size(egui::vec2(56.0, 22.0)),
                        )
                        .clicked()
                    {
                        if let Some(ref mut b) = self.board {
                            if self.streaming {
                                let _ = b.stop_streaming();
                                self.streaming = false;
                                self.event_log.log_system("Streaming stopped");
                            } else if let Err(e) = b.start_streaming() {
                                tracing::error!("Start failed: {:?}", e);
                                self.event_log
                                    .log_error(&format!("Failed to start streaming: {}", e));
                            } else {
                                self.streaming = true;
                                self.event_log.log_system("Streaming started");
                            }
                        }
                    }

                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("End").color(theme::TEXT))
                                .fill(theme::PANEL)
                                .stroke(theme::hairline())
                                .min_size(egui::vec2(44.0, 22.0)),
                        )
                        .clicked()
                    {
                        self.end_session();
                    }

                    self.draw_record_export(ui);

                    let exp_running = self.experiment.is_running();
                    let exp_label = if exp_running {
                        "Stop experiment"
                    } else {
                        "Run experiment"
                    };
                    let live_hw = self.board.as_ref().is_some_and(|b| {
                        let n = b.name();
                        n != "Playback" && !n.contains("Synthetic")
                    });
                    let exp_fill = if exp_running {
                        theme::STOP
                    } else if live_hw {
                        theme::START
                    } else {
                        theme::PANEL
                    };
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(exp_label).color(theme::TEXT))
                                .fill(exp_fill)
                                .stroke(theme::hairline())
                                .min_size(egui::vec2(120.0, 22.0)),
                        )
                        .clicked()
                    {
                        if exp_running {
                            self.cancel_experiment();
                        } else {
                            self.start_experiment();
                        }
                    }

                    if self
                        .board
                        .as_ref()
                        .and_then(|b| b.playback_progress())
                        .is_some()
                    {
                        if ui.button("-10s").clicked() {
                            if let Some(ref mut b) = self.board {
                                if let Some((pos, total)) = b.playback_progress() {
                                    let sr = b.sample_rate().max(1) as f32;
                                    let tot = total.max(1) as f32;
                                    b.seek_to_fraction(
                                        ((pos as f32 - 10.0 * sr) / tot).clamp(0.0, 1.0),
                                    );
                                }
                            }
                        }
                        if ui.button("+10s").clicked() {
                            if let Some(ref mut b) = self.board {
                                if let Some((pos, total)) = b.playback_progress() {
                                    let sr = b.sample_rate().max(1) as f32;
                                    let tot = total.max(1) as f32;
                                    b.seek_to_fraction(
                                        ((pos as f32 + 10.0 * sr) / tot).clamp(0.0, 1.0),
                                    );
                                }
                            }
                        }
                        if ui.button("Play again").clicked() {
                            if let Some(ref mut b) = self.board {
                                b.seek_to_fraction(0.0);
                                if !b.is_streaming() {
                                    b.toggle_playback_pause();
                                }
                                self.streaming = true;
                            }
                        }
                    }

                    ui.separator();

                    if let Some(ref b) = self.board {
                        let run = if self.streaming { "live" } else { "stop" };
                        ui.label(
                            egui::RichText::new(format!("{}  {run}", b.name())).color(theme::TEXT),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  {}",
                                crate::stream_stats::format_hz(self.current_sample_rate).trim(),
                                crate::stream_stats::format_loss(self.packet_loss_percent).trim()
                            ))
                            .monospace()
                            .color(crate::stream_stats::loss_color(self.packet_loss_percent)),
                        );
                    }

                    if self.data_logger.is_logging() {
                        let dur = self
                            .data_logger
                            .recording_duration()
                            .map(|d| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60))
                            .unwrap_or_default();
                        ui.colored_label(theme::STOP, format!("REC {dur}"));
                    }

                    if let Some(focus) = self
                        .tool_widgets
                        .iter()
                        .find_map(|t| t.as_any().downcast_ref::<WFocus>())
                    {
                        ui.separator();
                        focus.paint_transport_chip(ui);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(
                            egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .color(theme::HAIRLINE),
                        );
                    });
                });
            });

        if !self.tool_widgets.is_empty() {
            egui::SidePanel::right("tool_panel")
                .resizable(true)
                .default_width(280.0)
                .min_width(220.0)
                .max_width(420.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::PANEL)
                        .stroke(theme::hairline())
                        .inner_margin(6.0),
                )
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.small(egui::RichText::new("PROPERTIES").color(theme::HAIRLINE));
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                let mut open = self.properties_open.take();
                                properties_card(ui, |ui| {
                                    self.draw_session_rack(ui, &mut open);
                                });
                                draw_exclusive_section(ui, &mut open, "Experiments", |ui| {
                                    ui.label(
                                        egui::RichText::new("Guided recording").color(theme::TEXT),
                                    );
                                    if let Some(board) = self.board.as_deref() {
                                        let mut widget_ctx = WidgetContext::new(
                                            &mut self.networking,
                                            &mut self.data_logger,
                                            &mut self.last_marker,
                                            &mut self.event_log,
                                            &mut self.emg,
                                        );
                                        ui.small(
                                            egui::RichText::new("Marker").color(theme::HAIRLINE),
                                        );
                                        show_named_tool(
                                            &mut self.tool_widgets,
                                            "Marker",
                                            ui,
                                            board,
                                            &mut widget_ctx,
                                        );
                                    }
                                });
                                draw_exclusive_section(ui, &mut open, "Networking", |ui| {
                                    if let Some(board) = self.board.as_deref() {
                                        let mut widget_ctx = WidgetContext::new(
                                            &mut self.networking,
                                            &mut self.data_logger,
                                            &mut self.last_marker,
                                            &mut self.event_log,
                                            &mut self.emg,
                                        );
                                        show_named_tool(
                                            &mut self.tool_widgets,
                                            "Networking",
                                            ui,
                                            board,
                                            &mut widget_ctx,
                                        );
                                    }
                                });
                                draw_exclusive_section(ui, &mut open, "Hardware", |ui| {
                                    // Headset and Montage controls (moved from Head Plot)
                                    ui.small(
                                        egui::RichText::new("Headset").color(theme::HAIRLINE),
                                    );
                                    let mut headset = HEADSET_NAME.to_string();
                                    egui::ComboBox::from_id_salt("hw_headset")
                                        .selected_text(HEADSET_NAME)
                                        .width(168.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut headset,
                                                HEADSET_NAME.to_string(),
                                                HEADSET_NAME,
                                            );
                                        });
                                    let _ = headset;

                                    // Montage profile selector and save controls
                                    ui.small(
                                        egui::RichText::new("Montage").color(theme::HAIRLINE),
                                    );
                                    // Get current montage state from WHeadPlot
                                    let (profile_name, profile_names, dirty, save_as_open) = {
                                        let hp = self
                                            .widget_manager
                                            .widgets
                                            .iter()
                                            .chain(self.tool_widgets.iter())
                                            .find_map(|w| w.as_any().downcast_ref::<WHeadPlot>());
                                        if let Some(hp) = hp {
                                            (
                                                hp.profile_name().to_string(),
                                                hp.profile_names().to_vec(),
                                                hp.is_dirty(),
                                                hp.is_save_as_open(),
                                            )
                                        } else {
                                            (
                                                self.montage.last_name().to_string(),
                                                self.montage.names(),
                                                false,
                                                false,
                                            )
                                        }
                                    };
                                    let shown = if dirty {
                                        format!("{}*", profile_name)
                                    } else {
                                        profile_name.clone()
                                    };
                                    let mut pick = profile_name.clone();
                                    ui.horizontal(|ui| {
                                        egui::ComboBox::from_id_salt("hw_montage_profile")
                                            .selected_text(&shown)
                                            .width(110.0)
                                            .show_ui(ui, |ui| {
                                                for n in &profile_names {
                                                    ui.selectable_value(&mut pick, n.clone(), n);
                                                }
                                            });
                                        let save = egui::Button::new("Save").small().frame(false);
                                        if ui
                                            .add(save)
                                            .on_hover_text(
                                                "Write channel → 10-20 hole into the active profile",
                                            )
                                            .clicked()
                                        {
                                            for w in self
                                                .widget_manager
                                                .widgets
                                                .iter_mut()
                                                .chain(self.tool_widgets.iter_mut())
                                            {
                                                if let Some(hp) =
                                                    w.as_any_mut().downcast_mut::<WHeadPlot>()
                                                {
                                                    hp.set_action(MontageUiAction::Save);
                                                }
                                            }
                                        }
                                        if ui
                                            .add(egui::Button::new("Save as").small().frame(false))
                                            .clicked()
                                        {
                                            for w in self
                                                .widget_manager
                                                .widgets
                                                .iter_mut()
                                                .chain(self.tool_widgets.iter_mut())
                                            {
                                                if let Some(hp) =
                                                    w.as_any_mut().downcast_mut::<WHeadPlot>()
                                                {
                                                    hp.set_save_as_open(true);
                                                }
                                            }
                                        }
                                    });
                                    if pick != profile_name {
                                        for w in self
                                            .widget_manager
                                            .widgets
                                            .iter_mut()
                                            .chain(self.tool_widgets.iter_mut())
                                        {
                                            if let Some(hp) =
                                                w.as_any_mut().downcast_mut::<WHeadPlot>()
                                            {
                                                hp.set_action(MontageUiAction::Select(pick.clone()));
                                            }
                                        }
                                    }
                                    // Save as dialog
                                    if save_as_open {
                                        ui.horizontal(|ui| {
                                            ui.label("Name");
                                            let mut buf = String::new();
                                            for w in self
                                                .widget_manager
                                                .widgets
                                                .iter_mut()
                                                .chain(self.tool_widgets.iter_mut())
                                            {
                                                if let Some(hp) =
                                                    w.as_any_mut().downcast_mut::<WHeadPlot>()
                                                {
                                                    buf = hp.save_as_buf().to_string();
                                                    break;
                                                }
                                            }
                                            let resp = ui.add(
                                                egui::TextEdit::singleline(&mut buf)
                                                    .desired_width(140.0),
                                            );
                                            // Update buffer in WHeadPlot
                                            for w in self
                                                .widget_manager
                                                .widgets
                                                .iter_mut()
                                                .chain(self.tool_widgets.iter_mut())
                                            {
                                                if let Some(hp) =
                                                    w.as_any_mut().downcast_mut::<WHeadPlot>()
                                                {
                                                    *hp.save_as_buf_mut() = buf.clone();
                                                }
                                            }
                                            if ui.button("Create").clicked()
                                                || (resp.lost_focus()
                                                    && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                            {
                                                let name = buf.trim().to_string();
                                                if !name.is_empty() {
                                                    for w in self
                                                        .widget_manager
                                                        .widgets
                                                        .iter_mut()
                                                        .chain(self.tool_widgets.iter_mut())
                                                    {
                                                        if let Some(hp) =
                                                            w.as_any_mut().downcast_mut::<WHeadPlot>()
                                                        {
                                                            hp.set_action(MontageUiAction::SaveAs(
                                                                name.clone(),
                                                            ));
                                                            hp.set_save_as_open(false);
                                                            hp.save_as_buf_mut().clear();
                                                        }
                                                    }
                                                }
                                            }
                                            if ui.button("Cancel").clicked() {
                                                for w in self
                                                    .widget_manager
                                                    .widgets
                                                    .iter_mut()
                                                    .chain(self.tool_widgets.iter_mut())
                                                {
                                                    if let Some(hp) =
                                                        w.as_any_mut().downcast_mut::<WHeadPlot>()
                                                    {
                                                        hp.set_save_as_open(false);
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    ui.add_space(8.0);

                                    if let Some(board) = self.board.as_deref() {
                                        {
                                            let mut widget_ctx = WidgetContext::new(
                                                &mut self.networking,
                                                &mut self.data_logger,
                                                &mut self.last_marker,
                                                &mut self.event_log,
                                                &mut self.emg,
                                            );
                                            ui.small(
                                                egui::RichText::new("Board").color(theme::HAIRLINE),
                                            );
                                            show_named_tool(
                                                &mut self.tool_widgets,
                                                "Board",
                                                ui,
                                                board,
                                                &mut widget_ctx,
                                            );
                                            ui.small(
                                                egui::RichText::new("Impedance")
                                                    .color(theme::HAIRLINE),
                                            );
                                            show_named_tool(
                                                &mut self.tool_widgets,
                                                "Impedance",
                                                ui,
                                                board,
                                                &mut widget_ctx,
                                            );
                                            ui.small(
                                                egui::RichText::new("Analog Read")
                                                    .color(theme::HAIRLINE),
                                            );
                                            show_named_tool(
                                                &mut self.tool_widgets,
                                                "Analog Read",
                                                ui,
                                                board,
                                                &mut widget_ctx,
                                            );
                                            ui.small(
                                                egui::RichText::new("Digital Read")
                                                    .color(theme::HAIRLINE),
                                            );
                                            show_named_tool(
                                                &mut self.tool_widgets,
                                                "Digital Read",
                                                ui,
                                                board,
                                                &mut widget_ctx,
                                            );
                                            ui.small(
                                                egui::RichText::new("Pulse Sensor")
                                                    .color(theme::HAIRLINE),
                                            );
                                            show_named_tool(
                                                &mut self.tool_widgets,
                                                "Pulse Sensor",
                                                ui,
                                                board,
                                                &mut widget_ctx,
                                            );
                                        }
                                        // Packet Loss inspect lives in Hardware, not as a spine row.
                                        ui.small(
                                            egui::RichText::new("Packet Loss")
                                                .color(theme::HAIRLINE),
                                        );
                                        let loss = self.packet_loss_percent;
                                        let loss_color = crate::stream_stats::loss_color(loss);
                                        ui.horizontal(|ui| {
                                            ui.colored_label(loss_color, format!("{:.1}%", loss));
                                            if ui.button("Reset").clicked() {
                                                self.packet_loss_history.clear();
                                                self.packet_loss_percent = 0.0;
                                                self.window_samples = 0;
                                                self.window_lost = 0;
                                                self.last_sample_time = None;
                                                self.samples_received = 0;
                                                self.event_log
                                                    .log_system("Packet loss stats reset by user");
                                            }
                                        });
                                        let hist = &self.packet_loss_history;
                                        if !hist.is_empty() {
                                            let desired = egui::vec2(ui.available_width(), 42.0);
                                            let (resp, painter) =
                                                ui.allocate_painter(desired, egui::Sense::hover());
                                            let rect = resp.rect;
                                            let max_l = hist
                                                .iter()
                                                .copied()
                                                .fold(0.0f32, |a, b| a.max(b))
                                                .max(1.0);
                                            let n = hist.len() as f32;
                                            for (i, &v) in hist.iter().enumerate() {
                                                let x = rect.min.x + (i as f32 / n) * rect.width();
                                                let y_norm = (v / max_l).min(1.0);
                                                let y = rect.max.y - y_norm * rect.height();
                                                if i > 0 {
                                                    let px = rect.min.x
                                                        + ((i - 1) as f32 / n) * rect.width();
                                                    let py_norm = (hist[i - 1] / max_l).min(1.0);
                                                    let py = rect.max.y - py_norm * rect.height();
                                                    painter.line_segment(
                                                        [egui::pos2(px, py), egui::pos2(x, y)],
                                                        egui::Stroke::new(1.5_f32, loss_color),
                                                    );
                                                }
                                            }
                                        } else {
                                            ui.small("(no loss history yet)");
                                        }
                                    }
                                });
                                draw_exclusive_section(ui, &mut open, "Fonts", |ui| {
                                    ui.label(
                                        egui::RichText::new("Type sizes").color(theme::TEXT),
                                    );
                                    let mut dirty = false;
                                    let mut drag = |ui: &mut egui::Ui, label: &str, val: &mut f32| {
                                        ui.horizontal(|ui| {
                                            ui.label(label);
                                            if ui
                                                .add(
                                                    egui::DragValue::new(val)
                                                        .range(8.0..=48.0)
                                                        .speed(0.25),
                                                )
                                                .changed()
                                            {
                                                dirty = true;
                                            }
                                        });
                                    };
                                    drag(ui, "small", &mut self.font_sizes.small);
                                    drag(ui, "body", &mut self.font_sizes.body);
                                    drag(ui, "button", &mut self.font_sizes.button);
                                    drag(ui, "heading", &mut self.font_sizes.heading);
                                    drag(ui, "mono", &mut self.font_sizes.mono);
                                    drag(ui, "marks", &mut self.font_sizes.marks);
                                    drag(ui, "hole_label", &mut self.font_sizes.hole_label);
                                    drag(ui, "caption", &mut self.font_sizes.caption);
                                    if ui.button("Reset defaults").clicked() {
                                        self.font_sizes = theme::FontSizes::default();
                                        dirty = true;
                                    }
                                    if dirty {
                                        theme::set_font_sizes(self.font_sizes.clone());
                                        self.font_sizes.apply_egui(ui.ctx());
                                        self.save_current_persisted_settings();
                                    }
                                });
                                self.properties_open = open;
                            });
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::CANVAS))
            .show(ctx, |ui| {
                // Phase 7: as_deref() yields Option<&dyn DataSource> — uniform for Playback + live boards
                if let Some(board) = self.board.as_deref() {
                    self.widget_manager.update(board);

                    let overlay = self.experiment.overlay(std::time::Instant::now());
                    for w in &mut self.widget_manager.widgets {
                        if let Some(ts) = w.as_any_mut().downcast_mut::<WTimeSeries>() {
                            ts.set_experiment_overlay(overlay.clone());
                        }
                    }
                    // Create a fresh WidgetContext for this frame. This gives every widget
                    // (especially Marker and the new configurable WNetworking) the ability
                    // to send markers, reconfigure networking, etc. in a clean, borrow-checker
                    // friendly way. This is the Phase 4 architectural foundation.
                    // Phase 7: also passes the EventLog so WConsole and all widgets can emit
                    // structured, filterable events for the live audit trail.
                    let mut widget_ctx = WidgetContext::new(
                        &mut self.networking,
                        &mut self.data_logger,
                        &mut self.last_marker,
                        &mut self.event_log,
                        &mut self.emg,
                    );
                    self.widget_manager.draw(ui, board, &mut widget_ctx);
                    self.drain_head_montage();
                    self.sync_head_plot_chrome();
                }
            });

        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(32.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::TRANSPORT)
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 12,
                        top: 4,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| {
                // 32px bar − 4 top − 8 bottom = 20px inner; keep Pause/speed on this line.
                ui.spacing_mut().interact_size.y = 16.0;
                ui.spacing_mut().button_padding = egui::vec2(6.0, 1.0);
                ui.horizontal(|ui| {
                    // Phase 7 Playback polish: compact interactive controls for the magical roundtrip.
                    // Lets the user pause, change speed, and scrub the exact recording they just made
                    // while Focus ML+audio, markers (sent during replay), Networking, and Console
                    // continue to work exactly as in the live session. This makes validation and
                    // neurofeedback rehearsal trivial without hardware.
                    if let Some(b) = self.board.as_deref_mut() {
                        if let Some((pos, total)) = b.playback_progress() {
                            // Pause / Play
                            let is_paused = !b.is_streaming();
                            let pause_label = if is_paused { "▶ Play" } else { "⏸ Pause" };
                            if ui.button(pause_label).clicked() {
                                b.toggle_playback_pause();
                                self.event_log.log_system(if is_paused {
                                    "Playback resumed"
                                } else {
                                    "Playback paused"
                                });
                            }

                            // Speed presets
                            for &s in &[0.5, 1.0, 2.0] {
                                let lbl = format!("{:.1}x", s);
                                if ui
                                    .selectable_label(
                                        b.playback_speed()
                                            .is_some_and(|cur| (cur - s).abs() < 0.01),
                                        lbl,
                                    )
                                    .clicked()
                                {
                                    b.set_playback_speed(s);
                                    self.event_log
                                        .log_system(&format!("Playback speed set to {}x", s));
                                }
                            }

                            // Progress text + manual seek slider (0..1)
                            let frac = if total > 0 {
                                pos as f32 / total as f32
                            } else {
                                0.0
                            };
                            let secs = pos as f64 / b.sample_rate().max(1) as f64;
                            let total_secs = total as f64 / b.sample_rate().max(1) as f64;
                            ui.label(format!("{:.1}/{:.1}s", secs, total_secs));

                            let mut new_frac = frac;
                            if ui
                                .add(egui::Slider::new(&mut new_frac, 0.0..=1.0).show_value(false))
                                .changed()
                            {
                                b.seek_to_fraction(new_frac);
                                self.event_log.log_system(&format!(
                                    "Playback seeked to {:.0}%",
                                    new_frac * 100.0
                                ));
                            }
                            ui.separator();
                        }
                    }

                    if self.networking.has_active_streams() {
                        ui.colored_label(egui::Color32::from_rgb(100, 180, 255), "📡 Net");
                    }

                    if !self.last_marker.is_empty() {
                        ui.colored_label(theme::ACCENT, format!("Last: {}", self.last_marker));
                    }

                    if ui.button("Console").clicked() {
                        self.console_show_window = !self.console_show_window;
                    }
                    ui.separator();
                    if let Some(entry) = self.event_log.last_n(1).first() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&entry.message).color(entry.level.color()),
                            )
                            .truncate(),
                        );
                    }
                });
            });

        // Full Console window (filterable, searchable, live) — opens when user clicks the Console button
        if self.console_show_window {
            let mut open = self.console_show_window;
            egui::Window::new("Event Log — Console (Phase 7)")
                .open(&mut open)
                .default_size([720.0, 420.0])
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Live Experiment Audit Trail");
                        ui.label(format!("({} total events)", self.event_log.len()));
                        if ui.button("Clear All").clicked() {
                            self.event_log.clear();
                            self.event_log.log_system("Console log cleared by user");
                        }
                        if ui.button("Copy Visible").clicked() {
                            // Phase 7 polish: actually copy the currently filtered log to system clipboard
                            let all_cats: std::collections::HashSet<_> =
                                self.console_categories.iter().cloned().collect();
                            let visible = self.event_log.filtered(&all_cats, &self.console_search);
                            let mut content = String::new();
                            content.push_str("# OpenBCI GUI — Session Event Log (visible)\n");
                            content.push_str(&format!("Copied: {}\n\n", chrono::Local::now()));
                            for e in &visible {
                                content.push_str(&format!(
                                    "[{:.3}] {:7} | {:12} | {}\n",
                                    e.timestamp,
                                    e.level.as_str(),
                                    e.category,
                                    e.message
                                ));
                            }
                            ui.ctx().copy_text(content.clone());
                            self.event_log.log_system(&format!(
                                "Copied {} visible events to clipboard",
                                visible.len()
                            ));
                        }
                        if ui.button("Save Log...").clicked() {
                            // Use rfd for native save dialog
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("openbci_session_log.txt")
                                .add_filter("Text", &["txt"])
                                .save_file()
                            {
                                // Write a simple text dump of current filtered log
                                let all_cats: std::collections::HashSet<_> =
                                    self.console_categories.iter().cloned().collect();
                                let visible =
                                    self.event_log.filtered(&all_cats, &self.console_search);
                                let mut content = String::new();
                                content.push_str("# OpenBCI GUI — Session Event Log\n");
                                content
                                    .push_str(&format!("Exported: {}\n\n", chrono::Local::now()));
                                for e in visible {
                                    content.push_str(&format!(
                                        "[{:.3}] {:7} | {:12} | {}\n",
                                        e.timestamp,
                                        e.level.as_str(),
                                        e.category,
                                        e.message
                                    ));
                                }
                                if let Err(e) = std::fs::write(&path, content) {
                                    self.event_log
                                        .log_error(&format!("Failed to save log: {}", e));
                                } else {
                                    self.event_log
                                        .log_system(&format!("Log saved to {}", path.display()));
                                }
                            }
                        }
                    });

                    ui.separator();

                    // Category filters
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Show:");
                        for cat in [
                            "Marker",
                            "Recording",
                            "Networking",
                            "Connection",
                            "Focus",
                            "Filter",
                            "System",
                            "Error",
                        ] {
                            let mut active = self.console_categories.contains(cat);
                            if ui.checkbox(&mut active, cat).changed() {
                                if active {
                                    self.console_categories.insert(cat.to_string());
                                } else {
                                    self.console_categories.remove(cat);
                                }
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Search:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.console_search)
                                .hint_text("filter by text..."),
                        );
                        if ui.button("✕").clicked() {
                            self.console_search.clear();
                        }
                    });

                    ui.separator();

                    // The actual scrollable log
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let filtered = self
                                .event_log
                                .filtered(&self.console_categories, &self.console_search);
                            if filtered.is_empty() {
                                ui.label("(no matching events)");
                            } else {
                                for entry in filtered {
                                    ui.horizontal(|ui| {
                                        // Timestamp
                                        ui.monospace(format!("{:.3}", entry.timestamp));
                                        // Level badge
                                        ui.colored_label(entry.level.color(), entry.level.as_str());
                                        // Category
                                        ui.strong(&entry.category);
                                        ui.label("—");
                                        // Message (selectable for copy)
                                        ui.add(egui::Label::new(&entry.message).selectable(true));
                                    });
                                    ui.add_space(1.0);
                                }
                            }
                        });
                });
            self.console_show_window = open;
        }

        if self.pending_layout_rebuild {
            self.rebuild_grid_widgets_for_current_layout();
            self.pending_layout_rebuild = false;
            self.apply_recapture_assign_holes();
        }

        ctx.request_repaint();
    }
}

fn properties_card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .stroke(theme::hairline())
        .inner_margin(8.0)
        .show(ui, add_contents);
    ui.add_space(8.0);
}

/// Montage Save/SaveAs always writes a live Head Plot map, even when `dirty`
/// was cleared. Prefer a dirty plot (the operator just rewired it); otherwise
/// the first plot (grid before the spare tool-panel copy).
pub(crate) fn pick_holes_for_montage_save(
    plots: &[([String; 8], bool)],
    fallback: [String; 8],
) -> [String; 8] {
    if let Some((holes, _)) = plots.iter().find(|(_, dirty)| *dirty) {
        return holes.clone();
    }
    plots
        .first()
        .map(|(holes, _)| holes.clone())
        .unwrap_or(fallback)
}

/// PROPERTIES accordion spine (Session = None; these ids = Some(id)).
pub(crate) const PROPERTIES_SPINE_IDS: &[&str] =
    &["Experiments", "Networking", "Hardware", "Fonts"];

fn show_named_tool(
    tools: &mut [Box<dyn Widget>],
    name: &str,
    ui: &mut egui::Ui,
    board: &dyn DataSource,
    ctx: &mut WidgetContext,
) {
    if let Some(tool) = tools.iter_mut().find(|t| t.title() == name) {
        tool.show(ui, board, ctx);
    }
}

/// PROPERTIES accordion: None = Session open; Some(id) = that section open, Session closed.
pub(crate) fn exclusive_section_open(current: &Option<String>, id: &str) -> bool {
    current.as_deref() == Some(id)
}

pub(crate) fn exclusive_section_clicked(current: &mut Option<String>, id: &str) {
    if current.as_deref() == Some(id) {
        *current = None;
    } else {
        *current = Some(id.to_string());
    }
}

fn draw_exclusive_section(
    ui: &mut egui::Ui,
    current: &mut Option<String>,
    id: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let is_open = exclusive_section_open(current, id);
    properties_card(ui, |ui| {
        let resp = egui::CollapsingHeader::new(id)
            .open(Some(is_open))
            .show(ui, add_contents);
        if resp.header_response.clicked() {
            exclusive_section_clicked(current, id);
        }
    });
}

#[cfg(test)]
mod properties_rack_tests {
    use super::{
        exclusive_section_clicked, exclusive_section_open, pick_holes_for_montage_save,
        PROPERTIES_SPINE_IDS,
    };
    use crate::widgets::head_plot::LABELS;
    use crate::widgets::Widget;

    fn holes(site: &str) -> [String; 8] {
        let mut m = LABELS.map(|s| s.to_string());
        m[2] = site.to_string();
        m
    }

    #[test]
    fn save_uses_live_map_even_when_not_dirty() {
        let live = holes("T7");
        let store = LABELS.map(|s| s.to_string());
        let got = pick_holes_for_montage_save(&[(live.clone(), false)], store);
        assert_eq!(got[2], "T7");
        assert_ne!(got[2], "C3");
    }

    #[test]
    fn save_prefers_dirty_plot_over_spare_clean_copy() {
        let dirty = holes("T7");
        let spare = LABELS.map(|s| s.to_string());
        let got = pick_holes_for_montage_save(
            &[(dirty.clone(), true), (spare, false)],
            LABELS.map(|s| s.to_string()),
        );
        assert_eq!(got[2], "T7");
    }

    #[test]
    fn save_grid_map_wins_when_neither_is_dirty() {
        let grid = holes("T7");
        let spare = LABELS.map(|s| s.to_string());
        let got = pick_holes_for_montage_save(
            &[(grid.clone(), false), (spare, false)],
            LABELS.map(|s| s.to_string()),
        );
        assert_eq!(got[2], "T7", "must not skip the live grid map when clean");
    }

    #[test]
    fn drain_head_montage_source_always_snapshots_channel_holes() {
        let src = include_str!("app.rs");
        assert!(src.contains("pick_holes_for_montage_save"));
        assert!(src.contains("hp.channel_holes()"));
        assert!(
            !src.contains("if hp.is_dirty() {\n                    live_labels"),
            "Save must not skip the live map when dirty is false"
        );
    }

    #[test]
    fn spine_names_are_experiments_networking_hardware() {
        assert_eq!(
            PROPERTIES_SPINE_IDS,
            &["Experiments", "Networking", "Hardware", "Fonts"]
        );
        for banned in [
            "Marker",
            "Focus",
            "Impedance",
            "Board",
            "Analog Read",
            "Digital Read",
            "Pulse Sensor",
            "Packet Loss",
            "Head Plot",
            "Left / right",
            "Which first",
            "Slow Waves",
            "Hemispheres",
            "Session",
        ] {
            assert!(
                !PROPERTIES_SPINE_IDS.contains(&banned),
                "{banned} must not be a spine id"
            );
        }
    }

    #[test]
    fn accordion_starts_with_none_open() {
        let current: Option<String> = None;
        for id in PROPERTIES_SPINE_IDS {
            assert!(
                !exclusive_section_open(&current, id),
                "{id} must not default-open"
            );
        }
    }

    #[test]
    fn opening_one_section_closes_the_other() {
        let mut current = None;
        exclusive_section_clicked(&mut current, "Experiments");
        assert!(exclusive_section_open(&current, "Experiments"));
        exclusive_section_clicked(&mut current, "Networking");
        assert!(!exclusive_section_open(&current, "Experiments"));
        assert!(exclusive_section_open(&current, "Networking"));
        assert!(!exclusive_section_open(&current, "Hardware"));
    }

    #[test]
    fn clicking_open_section_closes_it() {
        let mut current = Some("Hardware".into());
        exclusive_section_clicked(&mut current, "Hardware");
        assert!(current.is_none());
    }

    #[test]
    fn drain_head_montage_snapshots_holes_even_if_not_dirty() {
        let src = include_str!("app.rs");
        assert!(
            src.contains("Always snapshot the live Head Plot map"),
            "Save must persist channel_holes even when dirty was cleared"
        );
        let drain = src
            .split("fn drain_head_montage")
            .nth(1)
            .unwrap_or("")
            .split("fn set_layout")
            .next()
            .unwrap_or("");
        assert!(
            !drain.contains("if hp.is_dirty()"),
            "dirty gate must not skip Save snapshot"
        );
        assert!(drain.contains("hp.channel_holes()"));
    }

    #[test]
    fn fonts_section_is_on_properties_spine() {
        assert!(PROPERTIES_SPINE_IDS.contains(&"Fonts"));
        let src = include_str!("app.rs");
        assert!(src.contains("hole_label"));
        assert!(src.contains("font_sizes.marks"));
    }

    #[test]
    fn session_filter_copy_is_smooth_cutoff_not_inventor() {
        let src = include_str!("app.rs");
        assert!(src.contains("Smooth cutoff"), "missing Smooth cutoff");
        let banned = format!("ui.label(\"{}\"", "Butterworth");
        assert!(
            !src.contains(&banned),
            "inventor name must not be a Session label"
        );
    }

    #[test]
    fn headset_label_on_head_plot_picker_not_on_traces() {
        let head = include_str!("widgets/head_plot.rs");
        let app = include_str!("app.rs");
        assert!(head.contains("Ultracortex Mark IV"));
        assert!(
            app.contains("hw_headset"),
            "headset combo lives in Hardware inspect"
        );
        assert!(
            app.contains("\"Save as\""),
            "Save as lives in Hardware montage row"
        );
        assert!(
            !head.contains("head_headset"),
            "Head Plot chrome is Waves/Hemispheres only"
        );
        let session = app
            .split("fn draw_session_rack")
            .nth(1)
            .unwrap_or("")
            .split("impl eframe::App")
            .next()
            .unwrap_or("");
        assert!(
            !session.contains("Ultracortex"),
            "headset picker must not live on Session"
        );
        assert!(!PROPERTIES_SPINE_IDS.contains(&"Head Plot"));
        assert!(!PROPERTIES_SPINE_IDS.contains(&"Headset"));
    }

    #[test]
    fn session_view_pickers_are_head_plot_left_right_which_first() {
        let src = include_str!("app.rs");
        let slots = src
            .split("fn draw_layout_slots")
            .nth(1)
            .unwrap_or("")
            .split("fn draw_record_export")
            .next()
            .unwrap_or("");
        assert!(slots.contains("\"Head Plot\""), "{slots}");
        assert!(slots.contains("\"Left / right\""), "{slots}");
        assert!(slots.contains("\"Which first\""), "{slots}");
        assert!(!slots.contains("\"Hemispheres\""), "{slots}");
        assert!(!slots.contains("\"Slow Waves\""), "{slots}");
        let rebuild = src
            .split("fn rebuild_grid_widgets_for_current_layout")
            .nth(1)
            .unwrap_or("")
            .split("fn populate_widgets_for_new_session")
            .next()
            .unwrap_or("");
        assert!(rebuild.contains("\"Left / right\""));
        assert!(rebuild.contains("\"Which first\""));
        assert!(!rebuild.contains("\"Hemispheres\""));
        assert!(!rebuild.contains("\"Slow Waves\""));
        assert_eq!(crate::widgets::WHemispheres::new().title(), "Left / right");
        assert_eq!(crate::widgets::WSlowWaves::new().title(), "Which first");
        assert_eq!(crate::widgets::WHeadPlot::new().title(), "Head Plot");
    }

    #[test]
    fn session_is_open_when_none_closed_when_some() {
        let current: Option<String> = None;
        assert!(
            current.is_none(),
            "None = Session open, exclusive sections closed"
        );

        let current = Some("Experiments".into());
        assert!(exclusive_section_open(&current, "Experiments"));
        assert!(
            current.is_some(),
            "Some = Session closed, that exclusive section open"
        );

        let mut current = Some("Experiments".into());
        exclusive_section_clicked(&mut current, "Hardware");
        assert!(!exclusive_section_open(&current, "Experiments"));
        assert!(exclusive_section_open(&current, "Hardware"));
    }

    #[test]
    fn recapture_properties_env_is_documented() {
        let src = include_str!("app.rs");
        assert!(
            src.contains("OPENBCI_PROPERTIES"),
            "OPENBCI_PROPERTIES env var must be documented"
        );
        assert!(
            src.contains("apply_recapture_properties"),
            "recapture properties method must exist"
        );
        assert!(
            src.contains("OPENBCI_CROP"),
            "OPENBCI_PROPERTIES requires OPENBCI_CROP guard"
        );
        for section in PROPERTIES_SPINE_IDS {
            assert!(
                src.contains(section),
                "section {section} must be in PROPERTIES_SPINE_IDS"
            );
        }
    }

    #[test]
    fn session_start_not_gated_by_properties_accordion() {
        use super::{setup_panel_active, SystemMode};

        // Hardware open collapses Session in the rack, but must not block PostInit.
        assert!(!setup_panel_active(SystemMode::PostInit));
        assert!(setup_panel_active(SystemMode::PreInit));

        let properties_open = Some("Hardware".to_string());
        assert!(
            exclusive_section_open(&properties_open, "Hardware"),
            "accordion state is independent of session mode"
        );
        assert!(!setup_panel_active(SystemMode::PostInit));
    }

    #[test]
    fn setup_panel_fallthrough_after_start_is_in_update() {
        let src = include_str!("app.rs");
        assert!(
            src.contains("Session may have started this frame"),
            "PreInit must fall through to transport when session enters on the same frame"
        );
        assert!(
            src.contains("if setup_panel_active(self.system_mode)"),
            "setup panel early return must be conditional on still being PreInit"
        );
    }
}
