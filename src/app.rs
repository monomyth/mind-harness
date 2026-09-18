// OpenBCI GUI Application State
//
// Now driven by the Widget + WidgetManager system.

use crate::board::ads_settings::{default_bank, AdsChannel};
use crate::board::brainflow_board::BrainFlowBoard;
use crate::board::{recent_raw_rows, DataSource};
use crate::control_panel::{ControlPanel, DataSourceType};
use crate::data_logger::{RecordPump, RecordingSample};
use crate::experiment::ProtocolKind;
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

/// Place-locked shell (ReBot hierarchy). Interface owns place; soft until crop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppPlace {
    SessionSetup,
    Live,
    Playback,
}

impl AppPlace {
    pub fn label(self) -> &'static str {
        match self {
            Self::SessionSetup => "Session Setup",
            Self::Live => "Live",
            Self::Playback => "Playback",
        }
    }
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
            recording_format: crate::data_logger::LogFormat::Parquet,
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
    pub place: AppPlace,
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
    pub data_logger: RecordPump,
    pub networking: NetworkingManager,
    pub connection_status: String,
    pub recording_format: crate::data_logger::LogFormat,
    export_kind: crate::export::ExportKind,
    export_prompt_open: bool,
    /// Cyton on-board SD logging armed / active (SDK file on the card).
    cyton_sd_active: bool,
    /// When SD became active (REC elapsed for SD-only; local uses data_logger).
    cyton_sd_started_at: Option<std::time::Instant>,
    cyton_sd_duration: crate::board::cyton_sd_write::CytonSdDuration,
    record_destination: crate::board::cyton_sd_write::RecordDestination,
    scrubbing: bool,
    scrub_was_playing: bool,

    // Packet loss & sample rate tracking
    samples_received: u64,
    last_sample_time: Option<std::time::Instant>,
    current_sample_rate: f64,
    packet_loss_percent: f64,

    /// Phase 7: accumulator for samples delivered (via the new DataSource::recent_samples_delivered)
    /// within the current measurement window. Enables correct packet loss % for the SidePanel visual.
    window_samples: u64,
    window_lost: u64,
    window_empty: u64,
    starve: crate::starve::StarveLog,
    live_autostart_used: bool,
    /// True when this session is BrainFlow synthetic because no dongle (or Cyton failed).
    simulation_notice: bool,
    device_picker_open: bool,
    pending_device: Option<(DataSourceType, usize, Option<String>)>,

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
    experiment_protocol: ProtocolKind,
    last_spike_t: f64,
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
            place: AppPlace::SessionSetup,
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
            data_logger: RecordPump::spawn(),
            networking: NetworkingManager::new(),
            connection_status: String::new(),
            recording_format: crate::data_logger::LogFormat::Parquet,
            export_kind: crate::export::ExportKind::Bdf,
            export_prompt_open: false,
            cyton_sd_active: false,
            cyton_sd_started_at: None,
            cyton_sd_duration: crate::board::cyton_sd_write::CytonSdDuration::Min5,
            record_destination: crate::board::cyton_sd_write::RecordDestination::Local,
            scrubbing: false,
            scrub_was_playing: false,

            samples_received: 0,
            last_sample_time: None,
            current_sample_rate: 0.0,
            packet_loss_percent: 0.0,
            window_samples: 0,
            window_lost: 0,
            window_empty: 0,
            starve: crate::starve::StarveLog::default(),
            live_autostart_used: false,
            simulation_notice: false,
            device_picker_open: false,
            pending_device: None,

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
            experiment_protocol: ProtocolKind::Guided,
            last_spike_t: f64::NEG_INFINITY,
            contact: crate::contact::ContactLog::new(),
            montage: MontageStore::load(),
        };

        // === Phase 8: Load persisted settings (silent on any error / missing file) ===
        let persisted = OpenBciGuiApp::load_persisted_settings();
        app.control_panel.selected_source = persisted.selected_source;
        app.control_panel.show_advanced = persisted.selected_source.is_advanced();
        app.control_panel.synthetic_channels = persisted.synthetic_channels;
        app.control_panel.cyton_channels = persisted.cyton_channels;
        app.control_panel.playback_file = persisted.playback_file.clone();
        app.control_panel.cyton_wifi_ip = persisted.cyton_wifi_ip.clone();
        app.control_panel.ganglion_device_id = persisted.ganglion_device_id.clone();
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
        let mut live_holes: Option<[String; 8]> = None;
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
                if live_holes.is_none() {
                    live_holes = Some(hp.channel_holes());
                }
            }
        }
        // Time Series left column = live Head Plot map (channel_holes), else store.
        let ts_labels = live_holes.unwrap_or(holes);
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(ts) = w
                .as_any_mut()
                .downcast_mut::<crate::widgets::time_series::WTimeSeries>()
            {
                ts.set_channel_labels(ts_labels.clone());
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
        let Some((show_waves, show_hemispheres, holes)) = self
            .widget_manager
            .widgets
            .iter()
            .chain(self.tool_widgets.iter())
            .find_map(|w| {
                w.as_any().downcast_ref::<WHeadPlot>().map(|hp| {
                    (hp.show_waves, hp.show_hemispheres, hp.channel_holes())
                })
            })
        else {
            return;
        };
        if show_waves != self.head_show_waves || show_hemispheres != self.head_show_hemispheres {
            self.head_show_waves = show_waves;
            self.head_show_hemispheres = show_hemispheres;
            self.save_current_persisted_settings();
        }
        // Keep Time Series left labels on the live electrode map (channel_holes).
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(ts) = w
                .as_any_mut()
                .downcast_mut::<crate::widgets::time_series::WTimeSeries>()
            {
                ts.set_channel_labels(holes.clone());
            }
        }
    }

    fn drain_head_montage(&mut self) {
        // Always snapshot the live Head Plot map (even when dirty was cleared).
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
                "Left / right" | "Which first" => Box::new(WHeadPlot::new()),
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
        let is_take = self
            .board
            .as_ref()
            .and_then(|b| b.playback_progress())
            .is_some()
            || self
                .board
                .as_ref()
                .is_some_and(|b| b.name().contains("Playback"));
        self.place = if is_take {
            AppPlace::Playback
        } else {
            AppPlace::Live
        };
    }

    /// Phase 7 hybrid layout (plan.md Phase 7 step 5): populate the interactive tool widgets
    /// that live in the right SidePanel. These are always visible during a session.
    /// Focus (ML + audio) is now finally usable alongside the viz — the killer Phase 6 feature
    /// is no longer hidden. Marker and Networking controls are also always at hand.
    fn open_playback_file(&mut self, path: &str, seek_sec: f32) {
        // Play opens a recording. It never starts a live session.
        if self.data_logger.is_logging() {
            self.stop_recording_like_session();
        }
        if let Some(mut board) = self.board.take() {
            if board.is_streaming() {
                let _ = board.stop_streaming();
            }
            let _ = board.uninitialize();
        }
        self.streaming = false;
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

        // End session always stops Record if it is running.
        if self.data_logger.is_logging() {
            self.stop_recording_like_session();
            self.event_log
                .log_recording("Recording stopped (End Session)");
        }
        self.stop_cyton_sd_write_if_active();

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
        self.window_empty = 0;
        self.last_sample_time = None;
        self.starve.reset();

        // Fresh viz + tools for next session (Phase 7 hybrid layout).
        self.populate_widgets_for_new_session();
        self.populate_tool_widgets();

        self.control_panel.show = true;
        self.live_autostart_used = false;
        self.simulation_notice = false;
        self.device_picker_open = false;
        self.system_mode = SystemMode::PreInit;
        self.place = AppPlace::SessionSetup;
        if let Some(ref p) = self.last_recording_path {
            if p.exists() {
                self.control_panel.playback_file = Some(p.display().to_string());
            }
        }

        self.event_log
            .log_system("Session ended — Session Setup");

        tracing::info!("Session ended — Session Setup");
    }

    /// Open the recording file on the writer thread. Never BoardShim / ingest RPC.
    fn start_recording_like_session(&mut self) -> bool {
        let chans = self
            .board
            .as_ref()
            .map(|b| b.exg_channels().len())
            .unwrap_or(8);
        let sr = self.board.as_ref().map(|b| b.sample_rate()).unwrap_or(250);
        let (n_analog, n_digital) = match self.board.as_deref() {
            Some(b) if b.cyton_board_mode() == Some(2) => (b.analog_channels().len(), 0usize),
            Some(b) if b.cyton_board_mode() == Some(3) => (0usize, b.digital_channels().len()),
            _ => (0, 0),
        };
        let fmt = self.recording_format;
        match self
            .data_logger
            .start_with_aux(fmt, chans, sr, n_analog, n_digital)
        {
            Ok(path) => {
                self.last_recording_path = Some(path.clone());
                self.connection_status = format!("Recording to {}", path.display());
                self.event_log.log_recording(&format!(
                    "Started {} → {}",
                    fmt.label(),
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

    fn stop_cyton_sd_write_if_active(&mut self) {
        if !self.cyton_sd_active {
            return;
        }
        if let Some(ref mut b) = self.board {
            match b.cyton_sd_write_stop() {
                Ok(()) => {
                    self.event_log.log_recording("Cyton SD write stopped");
                }
                Err(e) => {
                    self.event_log
                        .log_error(&format!("Cyton SD stop failed: {e}"));
                }
            }
        }
        self.cyton_sd_active = false;
        self.cyton_sd_started_at = None;
    }

    fn start_cyton_sd_write(&mut self) -> bool {
        // Fail-closed + bottom status only (Eugene lock / Interface).
        // No Hardware warn, under-control warn, or modal.
        const SD_STATUS_FAIL: &str = "Couldn't write to the SD card";
        let Some(ref mut b) = self.board else {
            self.cyton_sd_active = false;
            self.cyton_sd_started_at = None;
            self.connection_status = SD_STATUS_FAIL.to_string();
            self.event_log
                .log_error("Cyton SD write: start a Cyton session first");
            return false;
        };
        if !b.supports_cyton_sd_write() {
            self.cyton_sd_active = false;
            self.cyton_sd_started_at = None;
            self.connection_status = SD_STATUS_FAIL.to_string();
            self.event_log
                .log_error("Cyton SD write: this board cannot log to SD");
            return false;
        }
        // Both: no SD length UI — arm 24h silently. Sd: use Session combo.
        let dur = if self.record_destination
            == crate::board::cyton_sd_write::RecordDestination::Both
        {
            crate::board::cyton_sd_write::CytonSdDuration::Hour24
        } else {
            self.cyton_sd_duration
        };
        match b.cyton_sd_write_start(dur) {
            Ok(()) => {
                self.cyton_sd_active = true;
                self.cyton_sd_started_at = Some(std::time::Instant::now());
                self.event_log.log_recording(&format!(
                    "Cyton SD write started ({})",
                    dur.label()
                ));
                true
            }
            Err(e) => {
                // Never pretend SD is recording. Both may still Local-write.
                self.cyton_sd_active = false;
                self.cyton_sd_started_at = None;
                self.connection_status = SD_STATUS_FAIL.to_string();
                self.event_log
                    .log_error(&format!("Cyton SD start failed: {e}"));
                false
            }
        }
    }

    /// Rec elapsed — local disk duration, else SD-only Instant since arm.
    /// Both Rec follows local logging only (not SD-arm-from-Start).
    fn record_session_elapsed(&self) -> Option<std::time::Duration> {
        if self.data_logger.is_logging() {
            self.data_logger.recording_duration()
        } else if self.record_destination
            == crate::board::cyton_sd_write::RecordDestination::Sd
            && self.cyton_sd_active
        {
            self.cyton_sd_started_at.map(|t| t.elapsed())
        } else {
            None
        }
    }

    /// Rec 1:23 under an hour; H:MM:SS past hour.
    fn format_record_elapsed(d: std::time::Duration) -> String {
        let secs = d.as_secs();
        if secs >= 3600 {
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{}:{:02}", secs / 60, secs % 60)
        }
    }

    /// One transport Record — uses Session Record to Local|SD|Both.
    /// While streaming: never cyton_sd_write_start / config_board (SD arms on Start only).
    fn start_record_session(&mut self) {
        let dest = self.record_destination;
        // SD already active from Start: leave it. Mid-live dest change applies on next Start.
        if dest.wants_local() && !self.data_logger.is_logging() {
            let _ = self.start_recording_like_session();
        }
    }

    fn stop_record_session(&mut self) {
        if self.data_logger.is_logging() {
            self.stop_recording_like_session();
        }
        self.stop_cyton_sd_write_if_active();
    }

    fn tick_contact_sidecar(&mut self) {
        let (chs, sample, t_s, sr_hz, loss_pct, markers, path, accel, packet_index) = {
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
            let last = raw_rows.last();
            let accel_ch = board.accel_channels();
            let mut accel = [0.0_f64; 3];
            if let Some(row) = last {
                for (i, &c) in accel_ch.iter().take(3).enumerate() {
                    accel[i] = row.get(c).copied().unwrap_or(0.0);
                }
            }
            let packet_index = board
                .package_num_channel()
                .and_then(|i| last.and_then(|row| row.get(i).copied()))
                .unwrap_or(sample as f64) as u64;
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
            (chs, sample, t_s, sr_hz, loss_pct, markers, path, accel, packet_index)
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
        // Sidecar / console only — not the footer (All sites jumped is the plate).
        let _ = self.contact.take_common_mode_notice();
        if let Some((n_jump, mag)) = crate::laterality::common_mode_jump(&chs) {
            if n_jump >= 8 && t_s - self.last_spike_t > 1.0 {
                self.last_spike_t = t_s;
                if let Some(ref rec) = path {
                    let ev = crate::spikes::SpikeEvent {
                        t: t_s,
                        dv: crate::laterality::channel_max_steps(&chs),
                        mag,
                        accel,
                        packet_index,
                    };
                    let _ = crate::spikes::append(rec, &ev);
                }
            }
        }
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
            crate::experiment::ExperimentEvent::EnteredStep { index: _, label } => {
                if let Some(step) = self.experiment.current_step() {
                    crate::experiment::play_step_cue(step);
                    if step.hold.is_none() {
                        self.stop_recording_like_session();
                    }
                }
                if !label.is_empty() {
                    self.write_experiment_marker(&label);
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
        // Record/experiment start must not call BoardShim. Ingest stays on its
        // 4 ms pull; a Start RPC here hitching the worker is the Hertz cliff.
        if !self.streaming {
            self.connection_status = "Start streaming first".to_string();
            return;
        }
        if !self.data_logger.is_logging() && !self.start_recording_like_session() {
            return;
        }
        let ev = self
            .experiment
            .start_protocol(self.experiment_protocol, std::time::Instant::now());
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
        let is_take = self
            .board
            .as_ref()
            .and_then(|b| b.playback_progress())
            .is_some();
        ui.horizontal(|ui| {
            // Playback never offers Record — only live sessions do.
            if !is_take {
                // One Record only — destination is Session Record to Local|SD|Both.
                // SD|Both: SD arms on Start. Record toggles local when dest wants it;
                // SD-only: Stop Rec while cyton_sd_active (started on Start).
                let recording = if self.record_destination
                    == crate::board::cyton_sd_write::RecordDestination::Sd
                {
                    self.cyton_sd_active
                } else {
                    self.data_logger.is_logging()
                };
                let record_label = if recording { "Stop Rec" } else { "Record" };
                let live_hw = self.board.as_ref().is_some_and(|b| {
                    let n = b.name();
                    n != "Playback" && !n.contains("Synthetic")
                });
                let record_color = if recording {
                    theme::STOP
                } else if live_hw {
                    theme::START
                } else {
                    theme::PANEL
                };
                let mut record_btn = egui::Button::new(record_label).fill(record_color);
                if !live_hw && !recording {
                    record_btn = record_btn.stroke(theme::hairline());
                }
                if ui.add(record_btn).clicked() {
                    if recording {
                        self.stop_record_session();
                    } else {
                        self.start_record_session();
                    }
                }
                // Elapsed beside Record/Stop Rec (not Hertz/Loss). Both = local only.
                if recording {
                    if let Some(d) = self.record_session_elapsed() {
                        ui.colored_label(
                            theme::STOP,
                            format!("Rec {}", Self::format_record_elapsed(d)),
                        );
                    }
                }
                if !recording && self.record_destination.wants_local() {
                    egui::ComboBox::from_id_salt("record_format")
                        .selected_text(self.recording_format.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.recording_format,
                                crate::data_logger::LogFormat::Parquet,
                                "Parquet",
                            );
                            ui.selectable_value(
                                &mut self.recording_format,
                                crate::data_logger::LogFormat::Mcap,
                                "MCAP",
                            );
                            ui.selectable_value(
                                &mut self.recording_format,
                                crate::data_logger::LogFormat::BDF,
                                "BDF",
                            );
                            ui.selectable_value(
                                &mut self.recording_format,
                                crate::data_logger::LogFormat::ODF,
                                "OpenBCI text",
                            );
                        });
                }
            }
            if !self.data_logger.is_logging() {
                // Format is chosen in a prompt when Export is pressed — not chrome.
                if ui.button("Export").clicked() {
                    self.export_prompt_open = true;
                }
            }
        });
    }

    fn run_export_kind(&mut self, kind: crate::export::ExportKind) {
        self.export_kind = kind;
        self.export_prompt_open = false;
        let path = self.last_recording_path.clone().or_else(|| {
            self.control_panel
                .playback_file
                .as_ref()
                .map(std::path::PathBuf::from)
        });
        match path {
            Some(p) => match crate::export::export_recording(&p, kind) {
                Ok((out, extra)) => {
                    self.connection_status = format!("Exported {}", out.display());
                    if let Some(e) = extra {
                        self.event_log.log_recording(&format!(
                            "Export → {} / {}",
                            out.display(),
                            e.display()
                        ));
                    } else {
                        self.event_log.log_recording(&format!(
                            "Export {} → {}",
                            kind.label(),
                            out.display()
                        ));
                    }
                }
                Err(e) => {
                    self.event_log.log_error(&format!("Export failed: {e}"));
                }
            },
            None => {
                if let Some(picked) = rfd::FileDialog::new()
                    .set_title("Export recording")
                    .add_filter(
                        "Recordings",
                        &["parquet", "bdf", "odf", "txt", "csv"],
                    )
                    .pick_file()
                {
                    self.last_recording_path = Some(picked.clone());
                    self.control_panel.playback_file = Some(picked.display().to_string());
                    self.connection_status = format!("Export: chose {}", picked.display());
                    // Re-open prompt so format is chosen after the file pick.
                    self.export_prompt_open = true;
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

    /// Modal: pick export type when Export is pressed (not a standing combo).
    fn draw_export_prompt(&mut self, ctx: &egui::Context) {
        if !self.export_prompt_open {
            return;
        }
        let mut open = self.export_prompt_open;
        egui::Window::new("Export as")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(220.0);
                ui.label("Choose format for this export:");
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    if ui
                        .add_sized([200.0, 28.0], egui::Button::new("BDF"))
                        .clicked()
                    {
                        self.run_export_kind(crate::export::ExportKind::Bdf);
                    }
                    if ui
                        .add_sized([200.0, 28.0], egui::Button::new("OpenBCI text"))
                        .clicked()
                    {
                        self.run_export_kind(crate::export::ExportKind::OpenBciText);
                    }
                    if ui
                        .add_sized([200.0, 28.0], egui::Button::new("MCAP"))
                        .clicked()
                    {
                        self.run_export_kind(crate::export::ExportKind::Mcap);
                    }
                    if ui
                        .add_sized([200.0, 28.0], egui::Button::new("Features"))
                        .clicked()
                    {
                        self.run_export_kind(crate::export::ExportKind::Features);
                    }
                    ui.add_space(4.0);
                    if ui
                        .add_sized([200.0, 24.0], egui::Button::new("Cancel"))
                        .clicked()
                    {
                        self.export_prompt_open = false;
                    }
                });
            });
        if !open {
            self.export_prompt_open = false;
        }
    }

    /// Drag the playhead through the take (not only −10/+10). Pauses while dragging.
    fn apply_playback_scrub(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some((pos, total)) = self.board.as_ref().and_then(|b| b.playback_progress()) else {
            return;
        };
        if total == 0 {
            return;
        }
        let frac = pos as f32 / total as f32;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(width.max(64.0), 14.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, theme::PANEL);
        painter.rect_stroke(rect, 2.0, theme::hairline(), egui::StrokeKind::Inside);
        let x = rect.left() + frac.clamp(0.0, 1.0) * rect.width();
        painter.line_segment(
            [
                egui::pos2(x, rect.top() + 1.0),
                egui::pos2(x, rect.bottom() - 1.0),
            ],
            egui::Stroke::new(2.0_f32, theme::ACCENT),
        );
        let dragging = resp.dragged() || resp.is_pointer_button_down_on();
        if let Some(pointer) = resp.interact_pointer_pos() {
            if dragging || resp.clicked() {
                let t = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                if let Some(b) = self.board.as_deref_mut() {
                    if dragging && !self.scrubbing {
                        self.scrubbing = true;
                        self.scrub_was_playing = b.is_streaming();
                        if b.is_streaming() {
                            b.toggle_playback_pause();
                        }
                    }
                    b.seek_to_fraction(t);
                }
            }
        }
        if resp.drag_stopped() {
            if let Some(b) = self.board.as_deref_mut() {
                if self.scrub_was_playing && !b.is_streaming() {
                    b.toggle_playback_pause();
                }
            }
            self.scrubbing = false;
            self.scrub_was_playing = false;
        }
    }

    fn drain_time_series_drop_mark(&mut self) {
        let pending = self.widget_manager.widgets.iter_mut().find_map(|w| {
            w.as_any_mut()
                .downcast_mut::<WTimeSeries>()
                .and_then(|ts| ts.take_pending_drop_mark())
        });
        let Some(idx) = pending else {
            return;
        };
        if let Some(b) = self.board.as_deref_mut() {
            let label = crate::widgets::time_series::next_drop_mark_label(b.session_markers().len());
            b.drop_session_mark(idx, &label);
            self.last_marker = label;
        }
    }

    fn drain_time_series_scrub(&mut self) {
        let delta = self.widget_manager.widgets.iter_mut().find_map(|w| {
            w.as_any_mut()
                .downcast_mut::<WTimeSeries>()
                .and_then(|ts| ts.take_pending_scrub_delta())
        });
        let Some(delta) = delta else {
            return;
        };
        if let Some(b) = self.board.as_deref_mut() {
            if let Some((pos, total)) = b.playback_progress() {
                if total > 0 {
                    b.seek_to_fraction(pos as f32 / total as f32 + delta);
                }
            }
        }
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


                // Eugene: Record to destination lives in Session (not Hardware).
                // One transport Record; Local|SD|Both here. Cyton SD hex opens via Playback (one finished-take path).
                // Finished take (same as transport hiding Record): do not draw Record to / segment / SD length.
                let is_finished_take = self
                    .board
                    .as_ref()
                    .and_then(|b| b.playback_progress())
                    .is_some()
                    || self
                        .board
                        .as_ref()
                        .is_some_and(|b| b.name().contains("Playback"));
                if !is_finished_take {
                    ui.add_space(6.0);
                    ui.label("Record to");
                    ui.horizontal(|ui| {
                        for d in crate::board::cyton_sd_write::RecordDestination::ALL {
                            let selected = self.record_destination == d;
                            let mut btn = egui::Button::new(d.label())
                                .min_size(egui::vec2(52.0, 22.0));
                            if selected {
                                btn = btn.fill(theme::START);
                            } else {
                                btn = btn.fill(theme::PANEL).stroke(theme::hairline());
                            }
                            if ui.add(btn).clicked() {
                                // Mid-live dest change does not arm SD — applies on next Start.
                                self.record_destination = d;
                            }
                        }
                    });
                    // SD length combo ONLY for Sd — hide for Both/Local.
                    // Both arms Hour24 silently; status is "SD writing" (no duration).
                    let can_sd = self
                        .board
                        .as_ref()
                        .is_some_and(|b| b.supports_cyton_sd_write());
                    if self.record_destination
                        == crate::board::cyton_sd_write::RecordDestination::Sd
                    {
                        ui.horizontal(|ui| {
                            ui.label("SD length");
                            egui::ComboBox::from_id_salt("session_cyton_sd_duration")
                                .selected_text(self.cyton_sd_duration.label())
                                .width(80.0)
                                .show_ui(ui, |ui| {
                                    for d in crate::board::cyton_sd_write::CytonSdDuration::ALL {
                                        ui.selectable_value(
                                            &mut self.cyton_sd_duration,
                                            d,
                                            d.label(),
                                        );
                                    }
                                });
                        });
                        if !can_sd {
                            ui.small(
                                egui::RichText::new("Cyton session required for SD.")
                                    .color(theme::STOP),
                            );
                        } else if self.cyton_sd_active {
                            ui.small(
                                egui::RichText::new(format!(
                                    "SD writing · {}",
                                    self.cyton_sd_duration.label()
                                ))
                                .color(theme::STOP),
                            );
                        }
                    } else if self.record_destination
                        == crate::board::cyton_sd_write::RecordDestination::Both
                    {
                        if !can_sd {
                            ui.small(
                                egui::RichText::new("Cyton session required for SD.")
                                    .color(theme::STOP),
                            );
                        } else if self.cyton_sd_active {
                            ui.small(
                                egui::RichText::new("SD writing").color(theme::STOP),
                            );
                        }
                    }
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

    fn apply_display_controls(&mut self) {
        let window = self.persisted_ts_time_window_sec;
        let smooth = self.persisted_fft_smoothing_index;
        for w in self
            .widget_manager
            .widgets
            .iter_mut()
            .chain(self.tool_widgets.iter_mut())
        {
            if let Some(ts) = w.as_any_mut().downcast_mut::<WTimeSeries>() {
                ts.set_time_window(window);
            }
            if let Some(fft) = w.as_any_mut().downcast_mut::<WFFT>() {
                fft.set_window_sec(window);
                fft.set_smoothing_index(smooth);
            }
            if let Some(bp) = w.as_any_mut().downcast_mut::<WBandPower>() {
                bp.set_window_sec(window);
                bp.set_smoothing_index(smooth);
            }
            if let Some(sp) = w.as_any_mut().downcast_mut::<WSpectrogram>() {
                sp.set_window_sec(window);
                sp.set_smoothing_index(smooth);
            }
            if let Some(ac) = w.as_any_mut().downcast_mut::<WAccelerometer>() {
                ac.set_window_sec(window);
                ac.set_smoothing_index(smooth);
            }
        }
    }

    fn draw_display_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Window").color(theme::TEXT));
        let mut win = self.persisted_ts_time_window_sec;
        egui::ComboBox::from_id_salt("top_window")
            .selected_text(crate::widgets::window_label(win))
            .show_ui(ui, |ui| {
                for &secs in crate::widgets::WINDOW_SECS {
                    if ui
                        .selectable_label(
                            (win - secs).abs() < 0.1,
                            crate::widgets::window_label(secs),
                        )
                        .clicked()
                    {
                        win = secs;
                    }
                }
            });
        if (win - self.persisted_ts_time_window_sec).abs() > 0.05 {
            self.persisted_ts_time_window_sec = win;
            self.apply_display_controls();
            self.save_current_persisted_settings();
        }

        ui.label(egui::RichText::new("Smooth").color(theme::TEXT));
        let mut sm = self.persisted_fft_smoothing_index;
        egui::ComboBox::from_id_salt("top_smooth")
            .selected_text(crate::widgets::smooth_label(sm))
            .show_ui(ui, |ui| {
                for i in 0..crate::widgets::SMOOTH_FACTORS.len() {
                    if ui
                        .selectable_label(sm == i, crate::widgets::smooth_label(i))
                        .clicked()
                    {
                        sm = i;
                    }
                }
            });
        if sm != self.persisted_fft_smoothing_index {
            self.persisted_fft_smoothing_index = sm;
            self.persisted_bp_smoothing_index = sm;
            self.apply_display_controls();
            self.save_current_persisted_settings();
        }
    }

    /// Start a board from Session Setup (or nav). Soft — same paths as former live_auto arms.
    fn start_from_setup(
        &mut self,
        source: DataSourceType,
        chans: usize,
        serial_port: Option<String>,
    ) {
        self.control_panel.selected_source = source;
        match source {
            DataSourceType::Synthetic => {
                if std::env::var("OPENBCI_CROP").is_err() {
                    self.simulation_notice = true;
                } else {
                    self.simulation_notice = false;
                }
                self.connection_status = "Using BrainFlow Synthetic Board".to_string();
                let mut board = BrainFlowBoard::synthetic(chans);
                let _ = board.initialize();
                self.board = Some(Box::new(board) as Box<dyn DataSource>);
                if self.record_destination.wants_sd() {
                    self.streaming = false;
                } else if let Some(ref mut b) = self.board {
                    if b.start_streaming().is_ok() {
                        self.streaming = true;
                    }
                }
                self.event_log
                    .log_connection("Connected to BrainFlow Synthetic board");
                self.save_last_connection();
                self.enter_running_session();
            }
            DataSourceType::CytonSerial => {
                self.simulation_notice = false;
                let default_port = if cfg!(target_os = "macos") {
                    "/dev/cu.usbserial-0000".to_string()
                } else {
                    "/dev/tty.usbserial-0000".to_string()
                };
                let port = serial_port.unwrap_or(default_port);
                let is_daisy = chans >= 16;
                let port_for_thread = port.clone();
                let (tx, rx) = oneshot::channel();
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
            }
            DataSourceType::CytonWifi => {
                self.simulation_notice = false;
                let ip = serial_port.unwrap_or_default();
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
                    let mut board = BrainFlowBoard::cyton_wifi(&ip_for_thread, daisy);
                    let result = board.initialize().map(|_| board).map_err(|e| e.to_string());
                    let _ = tx.send(result);
                });
                self.connection_state = ConnectionState::InProgress {
                    receiver: rx,
                    status_message: format!("Connecting to Cyton WiFi {}...", ip),
                };
            }
            DataSourceType::GanglionNative => {
                self.simulation_notice = false;
                let id = serial_port.unwrap_or_default();
                if id.trim().is_empty() {
                    self.control_panel.show = true;
                    self.control_panel.last_setup_error = Some(
                        "Ganglion: enter a MAC / device name. This is not Synthetic.".into(),
                    );
                    return;
                }
                let id_for_thread = id.clone();
                let (tx, rx) = oneshot::channel();
                std::thread::spawn(move || {
                    let mut board = BrainFlowBoard::ganglion_native(&id_for_thread);
                    let result = board.initialize().map(|_| board).map_err(|e| e.to_string());
                    let _ = tx.send(result);
                });
                self.connection_state = ConnectionState::InProgress {
                    receiver: rx,
                    status_message: format!("Connecting to Ganglion {}...", id),
                };
            }
            DataSourceType::Playback => {
                self.simulation_notice = false;
                let file_path = serial_port.unwrap_or_default();
                if file_path.is_empty() {
                    self.control_panel.last_setup_error =
                        Some("Choose a playback file first.".into());
                    return;
                }
                match crate::board::playback::PlaybackBoard::from_file(std::path::Path::new(
                    &file_path,
                )) {
                    Ok(mut pb) => {
                        let _ = pb.initialize();
                        if pb.start_streaming().is_ok() {
                            self.streaming = true;
                        }
                        self.board = Some(Box::new(pb) as Box<dyn DataSource>);
                        self.connection_status = format!("Playback: {}", file_path);
                        self.event_log.log_connection(&format!(
                            "Playback · {}",
                            std::path::Path::new(&file_path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("file"),
                        ));
                        self.save_last_connection();
                        self.enter_running_session();
                    }
                    Err(e) => {
                        self.control_panel.last_setup_error =
                            Some(format!("Failed to load playback file: {}", e));
                    }
                }
            }
        }
    }

    fn draw_rebot_shell(&mut self, ctx: &egui::Context) {
        // Sync place from session when board is up.
        if self.system_mode == SystemMode::PostInit {
            let is_take = self
                .board
                .as_ref()
                .and_then(|b| b.playback_progress())
                .is_some()
                || self
                    .board
                    .as_ref()
                    .is_some_and(|b| b.name().contains("Playback"));
            if self.place == AppPlace::SessionSetup {
                self.place = if is_take {
                    AppPlace::Playback
                } else {
                    AppPlace::Live
                };
            }
        }

        self.draw_nav_sidebar(ctx);

        match self.place {
            AppPlace::SessionSetup => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(theme::PANEL_DEEP))
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.label(
                                egui::RichText::new("Session Setup")
                                    .heading()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(12.0);
                            if let Some(result) = self.control_panel.draw(ui) {
                                self.pending_device = Some(result);
                            }
                        });
                    });
            }
            AppPlace::Live => {
                self.draw_live_inspector(ctx);
                self.draw_live_transport(ctx);
                self.draw_session_viewport(ctx);
            }
            AppPlace::Playback => {
                // Playback: viewport + scrub; no Record to inspector; quiet bottom status.
                self.draw_playback_transport(ctx);
                self.draw_session_viewport(ctx);
            }
        }
    }

    fn draw_nav_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("rebot_nav")
            .exact_width(220.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL_DEEP)
                    .inner_margin(egui::Margin::symmetric(10, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(theme::ACCENT_LIME, "●");
                    ui.label(egui::RichText::new("Mind Harness").strong().color(theme::TEXT));
                });
                ui.add_space(16.0);

                for place in [
                    AppPlace::SessionSetup,
                    AppPlace::Live,
                    AppPlace::Playback,
                ] {
                    let active = self.place == place;
                    let (bg, fg) = if active {
                        (theme::NAV_PILL, theme::ACCENT_LIME)
                    } else {
                        (theme::PANEL_DEEP, theme::TEXT_DIM)
                    };
                    let resp = ui.add(
                        egui::Button::new(egui::RichText::new(place.label()).color(fg))
                            .fill(bg)
                            .stroke(if active {
                                egui::Stroke::new(0.0, theme::ACCENT_LIME)
                            } else {
                                egui::Stroke::new(0.0, theme::PANEL_DEEP)
                            })
                            .min_size(egui::vec2(200.0, 28.0)),
                    );
                    if active {
                        let r = resp.rect;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(r.min.x, r.min.y + 4.0),
                                egui::vec2(3.0, r.height() - 8.0),
                            ),
                            1.0,
                            theme::ACCENT_LIME,
                        );
                    }
                    if resp.clicked() {
                        self.navigate_place(place);
                    }
                    ui.add_space(4.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(8.0);
                    let (dot, line) = self.sidebar_status_line();
                    ui.horizontal(|ui| {
                        ui.colored_label(dot, "●");
                        ui.label(egui::RichText::new(line).small().color(theme::TEXT_DIM));
                    });
                    if self.place == AppPlace::Live {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  {}",
                                crate::stream_stats::format_hz(self.current_sample_rate).trim(),
                                crate::stream_stats::format_loss(self.packet_loss_percent).trim()
                            ))
                            .small()
                            .monospace()
                            .color(crate::stream_stats::loss_color(self.packet_loss_percent)),
                        );
                    }
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
                });
            });
    }

    fn sidebar_status_line(&self) -> (egui::Color32, String) {
        match self.board.as_ref() {
            Some(b) => (theme::ACCENT_LIME, b.name().to_string()),
            None => (theme::TEXT_MUTED, "Offline".to_string()),
        }
    }

    fn navigate_place(&mut self, place: AppPlace) {
        match place {
            AppPlace::SessionSetup => {
                if self.system_mode == SystemMode::PostInit {
                    self.end_session();
                } else {
                    self.place = AppPlace::SessionSetup;
                }
            }
            AppPlace::Live => {
                if self.system_mode == SystemMode::PostInit {
                    let is_take = self
                        .board
                        .as_ref()
                        .and_then(|b| b.playback_progress())
                        .is_some();
                    if !is_take {
                        self.place = AppPlace::Live;
                    }
                }
                // Soft: no board yet — stay on Setup (do not invent a live empty cockpit).
            }
            AppPlace::Playback => {
                if self.system_mode == SystemMode::PostInit {
                    let is_take = self
                        .board
                        .as_ref()
                        .and_then(|b| b.playback_progress())
                        .is_some()
                        || self
                            .board
                            .as_ref()
                            .is_some_and(|b| b.name().contains("Playback"));
                    if is_take {
                        self.place = AppPlace::Playback;
                    }
                } else {
                    self.place = AppPlace::SessionSetup;
                    self.control_panel.selected_source = DataSourceType::Playback;
                }
            }
        }
    }

    fn draw_live_inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("rebot_inspector")
            .resizable(true)
            .default_width(300.0)
            .min_width(260.0)
            .max_width(360.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL_RAISED)
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        let mut open = self.properties_open.take();
                        properties_card(ui, |ui| {
                            self.draw_session_rack(ui, &mut open);
                        });
                        // Fail-closed contact/gate line only — no new chrome words.
                        if let Some(line) = self.contact.peek_notice() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(line).color(theme::STOP).small());
                        }
                        draw_exclusive_section(ui, &mut open, "Experiments", |ui| {
                            if !self.experiment.is_running() {
                                egui::ComboBox::from_id_salt("exp_protocol")
                                    .selected_text(self.experiment_protocol.label())
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.experiment_protocol,
                                            ProtocolKind::Guided,
                                            ProtocolKind::Guided.label(),
                                        );
                                        ui.selectable_value(
                                            &mut self.experiment_protocol,
                                            ProtocolKind::EyesClosed,
                                            ProtocolKind::EyesClosed.label(),
                                        );
                                    });
                            } else {
                                ui.label(
                                    egui::RichText::new(self.experiment.protocol().label())
                                        .color(theme::TEXT),
                                );
                            }
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
                                theme::PANEL_RAISED
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(exp_label).color(theme::TEXT),
                                    )
                                    .fill(exp_fill)
                                    .stroke(theme::hairline()),
                                )
                                .clicked()
                            {
                                if exp_running {
                                    self.cancel_experiment();
                                } else {
                                    self.start_experiment();
                                }
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
                                    .add(egui::Button::new("Load").small().frame(false))
                                    .on_hover_text("Load the selected profile")
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
                                            hp.set_action(MontageUiAction::Select(
                                                shown.clone(),
                                            ));
                                        }
                                    }
                                }
                                if ui
                                    .add(egui::Button::new("Default").small().frame(false))
                                    .on_hover_text("Official 8 inserts")
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
                                            hp.set_action(MontageUiAction::Select(
                                                crate::widgets::head_plot::DEFAULT_PROFILE_NAME
                                                    .to_string(),
                                            ));
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
                            ui.label(egui::RichText::new("Type sizes").color(theme::TEXT));
                            ui.add(
                                egui::Slider::new(&mut self.font_sizes.body, 12.0..=22.0)
                                    .text("Body"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.font_sizes.heading, 16.0..=28.0)
                                    .text("Heading"),
                            );
                            if ui.button("Reset fonts").clicked() {
                                self.font_sizes = theme::FontSizes::default();
                            }
                            theme::set_font_sizes(self.font_sizes.clone());
                        });
                        self.properties_open = open;
                    });
            });
    }

    fn draw_live_transport(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("rebot_transport")
            .exact_height(40.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::TRANSPORT)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let stream_label = transport_go_label(false, self.streaming);
                    let mut stream_btn =
                        egui::Button::new(egui::RichText::new(stream_label).color(theme::TEXT))
                            .min_size(egui::vec2(56.0, 24.0));
                    if self.streaming {
                        stream_btn = stream_btn.fill(theme::START);
                    } else {
                        stream_btn = stream_btn
                            .fill(theme::PANEL_RAISED)
                            .stroke(theme::hairline());
                    }
                    if ui.add(stream_btn).clicked() {
                        if self.streaming {
                            if let Some(ref mut b) = self.board {
                                let _ = b.stop_streaming();
                            }
                            self.streaming = false;
                            self.event_log.log_system("Streaming stopped");
                            self.stop_recording_like_session();
                            self.stop_cyton_sd_write_if_active();
                        } else {
                            let sd_ok = if self.record_destination.wants_sd()
                                && !self.cyton_sd_active
                            {
                                self.start_cyton_sd_write()
                            } else {
                                true
                            };
                            if sd_ok {
                                if let Some(ref mut b) = self.board {
                                    if b.start_streaming().is_ok() {
                                        self.streaming = true;
                                        self.event_log.log_system("Streaming started");
                                    }
                                }
                            }
                        }
                    }
                    self.draw_record_export(ui);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("End").color(theme::TEXT),
                                )
                                .fill(theme::PANEL_RAISED)
                                .stroke(theme::hairline())
                                .min_size(egui::vec2(44.0, 24.0)),
                            )
                            .clicked()
                        {
                            self.end_session();
                        }
                        if ui.button("Console").clicked() {
                            self.console_show_window = !self.console_show_window;
                        }
                        // Window / Smooth chips (already existed on transport).
                        self.draw_display_controls(ui);
                    });
                });
            });
    }

    fn draw_playback_transport(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("rebot_playback_bar")
            .exact_height(40.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::TRANSPORT)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let stream_label = transport_go_label(true, self.streaming);
                    let mut stream_btn =
                        egui::Button::new(egui::RichText::new(stream_label).color(theme::TEXT))
                            .min_size(egui::vec2(56.0, 24.0));
                    if self.streaming {
                        stream_btn = stream_btn.fill(theme::START);
                    } else {
                        stream_btn = stream_btn
                            .fill(theme::PANEL_RAISED)
                            .stroke(theme::hairline());
                    }
                    if ui.add(stream_btn).clicked() {
                        if let Some(ref mut b) = self.board {
                            if self.streaming {
                                let _ = b.stop_streaming();
                                self.streaming = false;
                            } else if b.start_streaming().is_ok() {
                                self.streaming = true;
                            }
                        }
                    }
                    self.apply_playback_scrub(ui, 220.0);
                    if let Some(b) = self.board.as_deref_mut() {
                        if b.playback_progress().is_some() {
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
                                }
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("End").color(theme::TEXT),
                                )
                                .fill(theme::PANEL_RAISED)
                                .stroke(theme::hairline()),
                            )
                            .clicked()
                        {
                            self.end_session();
                        }
                        if ui.button("Console").clicked() {
                            self.console_show_window = !self.console_show_window;
                        }
                    });
                });
            });
    }

    fn draw_session_viewport(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::CANVAS))
            .show(ctx, |ui| {
                self.apply_display_controls();
                if let Some(board) = self.board.as_deref() {
                    self.widget_manager.update(board);
                    let overlay = self.experiment.overlay(std::time::Instant::now());
                    for w in &mut self.widget_manager.widgets {
                        if let Some(ts) = w.as_any_mut().downcast_mut::<WTimeSeries>() {
                            ts.set_experiment_overlay(overlay.clone());
                        }
                    }
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
        self.drain_time_series_drop_mark();
        self.drain_time_series_scrub();
    }
}

impl eframe::App for OpenBciGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let _starve_ui = crate::starve::begin_ui_tick(ctx.input(|i| i.unstable_dt));
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
                    self.board = Some(Box::new(connected_board) as Box<dyn DataSource>); // plan.md Phase 7 — polymorphic board (Playback + Live)
                    // SD|Both: do not auto-stream — SD arms on transport Start before start_streaming.
                    // Local: auto-start so graphs appear immediately.
                    if self.record_destination.wants_sd() {
                        self.streaming = false;
                    } else if let Some(ref mut b) = self.board {
                        if let Err(e) = b.start_streaming() {
                            tracing::error!(
                                "Failed to auto-start streaming after hardware connection: {}",
                                e
                            );
                        } else {
                            self.streaming = true;
                        }
                    }
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
                    self.connection_state = ConnectionState::Idle;
                    self.simulation_notice = true;
                    self.device_picker_open = true;
                    self.connection_status =
                        "Couldn't open the board. Running a BrainFlow simulation.".to_string();
                    let chans = self.control_panel.synthetic_channels.max(1);
                    let mut board = BrainFlowBoard::synthetic(chans);
                    let _ = board.initialize();
                    self.board = Some(Box::new(board) as Box<dyn DataSource>);
                    if self.record_destination.wants_sd() {
                        self.streaming = false;
                    } else if let Some(ref mut b) = self.board {
                        if let Err(e) = b.start_streaming() {
                            tracing::error!("Failed to auto-start streaming on Synthetic: {}", e);
                        } else {
                            self.streaming = true;
                        }
                    }
                    self.event_log
                        .log_connection("Connected to BrainFlow Synthetic board (fallback)");
                    self.save_last_connection();
                    self.enter_running_session();
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
                                self.simulation_notice = true;
                                self.device_picker_open = true;
                                self.connection_status =
                                    "Connection cancelled. Running a BrainFlow simulation.".to_string();
                                let chans = self.control_panel.synthetic_channels.max(1);
                                let mut board = BrainFlowBoard::synthetic(chans);
                                let _ = board.initialize();
                                self.board = Some(Box::new(board) as Box<dyn DataSource>);
                                if let Some(ref mut b) = self.board {
                                    if b.start_streaming().is_ok() {
                                        self.streaming = true;
                                    }
                                }
                                self.event_log.log_connection(
                                    "Connected to BrainFlow Synthetic board (cancelled hardware)",
                                );
                                self.save_last_connection();
                                self.enter_running_session();
                            }
                        });
                    });
                    if matches!(self.connection_state, ConnectionState::InProgress { .. }) {
                        return;
                    }
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.connection_status = "Connection channel closed unexpectedly".to_string();
                    self.connection_state = ConnectionState::Idle;
                }
            }
        }

        if setup_panel_active(self.system_mode) {
            let live_auto = self.pending_device.take().or_else(|| if !self.live_autostart_used {
                    self.live_autostart_used = true;
                    if let Ok(port) = std::env::var("OPENBCI_LIVE_SERIAL") {
                        let port = port.trim().to_string();
                        if !port.is_empty() {
                            tracing::info!("OPENBCI_LIVE_SERIAL start on {port}");
                            Some((
                                crate::control_panel::DataSourceType::CytonSerial,
                                8usize,
                                Some(port),
                            ))
                        } else {
                            None
                        }
                    } else if std::env::var("OPENBCI_CROP").is_ok()
                        && std::env::var("OPENBCI_SYNTHETIC").is_ok()
                    {
                        self.simulation_notice = false;
                        Some((
                            crate::control_panel::DataSourceType::Synthetic,
                            8usize,
                            None,
                        ))
                    } else {
                        // Place lock: stay on Session Setup unless crop / LIVE_SERIAL.
                        tracing::info!("Session Setup — no blanket autostart");
                        None
                    }
                } else {
                    None
                });
                if let Some((source, chans, serial_port)) = live_auto {
                    self.control_panel.selected_source = source;
                    if source == DataSourceType::Synthetic {
                        if std::env::var("OPENBCI_CROP").is_err() {
                            self.simulation_notice = true;
                        }
                    } else {
                        self.simulation_notice = false;
                    }
                    match source {
                        DataSourceType::Synthetic => {
                            self.connection_status = "Using BrainFlow Synthetic Board".to_string();
                            let mut board = BrainFlowBoard::synthetic(chans);
                            let _ = board.initialize();
                            self.board = Some(Box::new(board) as Box<dyn DataSource>);
                            // SD|Both waits for transport Start (fail-closed if board cannot SD).
                            if self.record_destination.wants_sd() {
                                self.streaming = false;
                            } else if let Some(ref mut b) = self.board {
                                // Auto-start streaming so the user immediately sees graphs
                                if let Err(e) = b.start_streaming() {
                                    tracing::error!("Failed to auto-start streaming on Synthetic: {}", e);
                                } else {
                                    self.streaming = true;
                                }
                            }
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
                    }
                }
            // Session may have started this frame (Synthetic / Playback). Fall through to shell.
            if setup_panel_active(self.system_mode) {
                self.place = AppPlace::SessionSetup;
                self.draw_rebot_shell(ctx);
                if let Some((source, chans, serial_port)) = self.pending_device.take() {
                    self.live_autostart_used = true;
                    self.control_panel.selected_source = source;
                    self.start_from_setup(source, chans, serial_port);
                }
                if setup_panel_active(self.system_mode) {
                    ctx.request_repaint();
                    return;
                }
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
                let nominal_hz = b.sample_rate() as f64;
                if delivered_this_tick > 0 {
                    self.samples_received += delivered_this_tick;
                    self.window_samples += delivered_this_tick;
                    self.window_lost += lost_this_tick;
                } else {
                    self.window_empty += 1;
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
                        let is_playback = matches!(
                            self.control_panel.selected_source,
                            crate::control_panel::DataSourceType::Playback
                        );
                        if !is_playback {
                            self.starve.tick(
                                received_in_window,
                                lost_in_window,
                                elapsed,
                                self.window_empty,
                                nominal_hz,
                                self.data_logger.current_file().map(|p| p.as_path()),
                                b.last_ingest_error(),
                            );
                        }
                        self.last_sample_time = Some(now);
                        self.window_samples = 0;
                        self.window_lost = 0;
                        self.window_empty = 0;
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
                    self.window_empty = 0;
                    self.starve.note_window_start();
                }

                // Recording: raw EXG + Accel + packet index (+ analog/digital when on).
                // Enqueue only — writer thread waits on disk. Never the display-filtered buffer.
                if self.data_logger.is_logging() {
                    let exg = b.exg_channels().to_vec();
                    let accel = b.accel_channels().to_vec();
                    let pkg = b.package_num_channel();
                    let analog_idx = if b.cyton_board_mode() == Some(2) {
                        b.analog_channels().to_vec()
                    } else {
                        vec![]
                    };
                    let digital_idx = if b.cyton_board_mode() == Some(3) {
                        b.digital_channels().to_vec()
                    } else {
                        vec![]
                    };
                    let sr = b.sample_rate().max(1) as f64;
                    let latest = recent_raw_rows(b);
                    for row in latest {
                        let analog: Vec<f64> = analog_idx
                            .iter()
                            .map(|&c| row.get(c).copied().unwrap_or(0.0))
                            .collect();
                        let digital: Vec<f64> = digital_idx
                            .iter()
                            .map(|&c| row.get(c).copied().unwrap_or(0.0))
                            .collect();
                        let t = self.data_logger.samples_logged() as f64 / sr;
                        let rec = RecordingSample::from_board_row(
                            &row,
                            &exg,
                            &accel,
                            pkg,
                            self.data_logger.samples_logged(),
                        )
                        .with_aux(analog, digital)
                        .with_time(t);
                        self.data_logger.log_recording(&rec);
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

        // ReBot place-locked shell: sidebar | viewport | inspector | transport.
        self.draw_rebot_shell(ctx);

        self.draw_export_prompt(ctx);

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


fn transport_go_label(is_take: bool, streaming: bool) -> &'static str {
    match (is_take, streaming) {
        (true, true) => "Pause",
        (true, false) => "Play",
        (false, true) => "Stop",
        (false, false) => "Start",
    }
}

fn transport_run_chip(is_take: bool, streaming: bool) -> &'static str {
    match (is_take, streaming) {
        (true, true) => "play",
        (true, false) => "pause",
        (false, true) => "live",
        (false, false) => "stop",
    }
}

#[cfg(test)]
mod properties_rack_tests {
    use super::{
        exclusive_section_clicked, exclusive_section_open, pick_holes_for_montage_save,
        transport_go_label, transport_run_chip, PROPERTIES_SPINE_IDS,
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
            "Head Plot has no headset picker"
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
        assert!(!slots.contains("\"Left / right\""), "{slots}");
        assert!(!slots.contains("\"Which first\""), "{slots}");
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

    #[test]
    fn version_is_semver() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "2.3.1");
    }

    #[test]
    fn rebot_place_lock_shell_exists() {
        let src = include_str!("app.rs");
        assert!(src.contains("enum AppPlace"));
        assert!(src.contains("fn draw_nav_sidebar"));
        assert!(src.contains("fn draw_live_inspector"));
        assert!(src.contains("fn draw_live_transport"));
        assert!(src.contains("rebot_nav"));
        assert!(src.contains("ACCENT_LIME"));
        assert!(!src.contains("TopBottomPanel::top(\"top_nav\")"));
    }

    #[test]
    fn format_record_elapsed_under_and_past_hour() {
        use super::OpenBciGuiApp;
        assert_eq!(
            OpenBciGuiApp::format_record_elapsed(std::time::Duration::from_secs(83)),
            "1:23"
        );
        assert_eq!(
            OpenBciGuiApp::format_record_elapsed(std::time::Duration::from_secs(0)),
            "0:00"
        );
        assert_eq!(
            OpenBciGuiApp::format_record_elapsed(std::time::Duration::from_secs(3661)),
            "1:01:01"
        );
    }

    #[test]
    fn status_bar_is_allocated_before_central_panel() {
        let src = include_str!("app.rs");
        let status = src
            .find("TopBottomPanel::bottom(\"rebot_transport\")")
            .expect("live transport");
        let central = src
            .find("CentralPanel::default()\n            .frame(egui::Frame::NONE.fill(theme::CANVAS))")
            .expect("session central");
        assert!(
            status < central,
            "egui panels must be allocated before CentralPanel or every pane clips under the bar"
        );
    }
    #[test]
    fn status_bar_does_not_draw_a_second_scrub_bar() {
        let src = include_str!("app.rs");
        assert!(
            !src.contains(concat!("apply_playback_scrub(ui, ", "160.0)")),
            "bottom status must not host a second scrub bar"
        );
        assert!(
            src.contains("self.apply_playback_scrub(ui, 220.0)"),
            "playback transport scrubs in viewport bar"
        );
    }

    #[test]
    fn window_and_smooth_live_on_top_bar_not_session_cutoff() {
        let src = include_str!("app.rs");
        let transport = src
            .split("fn draw_live_transport")
            .nth(1)
            .unwrap_or("")
            .split("fn draw_playback_transport")
            .next()
            .unwrap_or("");
        assert!(
            transport.contains("draw_display_controls"),
            "{transport}"
        );
        assert!(src.contains("from_id_salt(\"top_window\")"));
        assert!(src.contains("from_id_salt(\"top_smooth\")"));
        let session = src
            .split("fn draw_session_rack")
            .nth(1)
            .unwrap_or("")
            .split("fn apply_display_controls")
            .next()
            .unwrap_or("");
        assert!(session.contains("Smooth cutoff"), "{session}");
        assert!(!session.contains("from_id_salt(\"top_window\")"));
        let ts = include_str!("widgets/time_series.rs");
        assert!(!ts.contains("from_id_salt(\"ts_window\")"));
        let fft = include_str!("widgets/fft.rs");
        assert!(!fft.contains("from_id_salt(\"fft_smooth\")"));
        let bp = include_str!("widgets/band_power.rs");
        assert!(!bp.contains("from_id_salt(\"bp_smooth\")"));
    }
    #[test]
    fn play_is_finished_take_live_is_start_stop() {
        assert_eq!(transport_go_label(false, false), "Start");
        assert_eq!(transport_go_label(false, true), "Stop");
        assert_eq!(transport_go_label(true, false), "Play");
        assert_eq!(transport_go_label(true, true), "Pause");
        assert_eq!(transport_run_chip(false, true), "live");
        assert_eq!(transport_run_chip(true, true), "play");
        let src = include_str!("app.rs");
        assert!(!src.contains(concat!("Play", " again")), "second Play is dead");
        assert!(!src.contains(concat!("RichText::new(\"Play\")")), "live transport must not offer Play");
        assert!(src.contains("Playback never offers Record"), "playback hides Record");
        assert!(src.contains("record_format"), "Parquet must be listed for Record");
        assert!(src.contains("LogFormat::Mcap"), "MCAP is a Record option");
        assert!(src.contains("Export as"), "Export format is a prompt, not chrome");
        assert!(src.contains("ExportKind::Mcap"), "MCAP is an Export option");
        assert!(
            src.contains("End sits at the far right of transport"),
            "End must be far right"
        );
        assert!(
            src.contains("start_record_session"),
            "one Record drives destination"
        );
        assert!(
            src.contains("Record to destination lives in Session"),
            "Record to Local|SD|Both lives in Session"
        );
        assert!(
            src.contains("ui.label(\"Record to\")"),
            "Record to label visible on Session (not HAIRLINE-only)"
        );
        assert!(
            !src.contains(concat!("Recording ", "destination")),
            "label is Record to, not the old destination chrome"
        );
        assert!(
            src.contains("do not draw Record to / segment / SD length"),
            "playback finished take hides Record to entirely"
        );
        assert!(
            src.contains("Couldn't write to the SD card"),
            "bottom status on SD fail"
        );
        assert!(
            src.contains("arm Cyton SD on Start, before start_streaming"),
            "SD|Both arms on Start before stream, not mid-Record"
        );
        assert!(
            src.contains("never mid-live Record via config_board"),
            "mid-live Record must not config_board for SD"
        );
        assert!(
            src.contains("Mid-live dest change does not arm SD"),
            "Record to mid-live applies on next Start"
        );
        assert!(
            src.contains("record_session_elapsed"),
            "Rec elapsed for local or SD-only"
        );
        assert!(
            src.contains("format_record_elapsed"),
            "Rec 1:23 under hour; H:MM:SS past hour"
        );
        assert!(
            src.contains("Elapsed beside Record/Stop Rec"),
            "Rec lives beside Record/Stop Rec in draw_record_export"
        );
        assert!(
            src.contains("not Hertz/Loss-adjacent"),
            "no REC chip beside Hertz/Loss"
        );
        assert!(
            !src.contains("format!(\"REC {dur}\")"),
            "old Hertz/Loss REC chip must be gone"
        );
        assert!(
            src.contains("SD length combo ONLY for Sd"),
            "SD length hidden for Both/Local"
        );
        assert!(
            src.contains("Both arms Hour24 silently"),
            "Both arms 24h without SD length combo"
        );
        assert!(
            src.contains("RichText::new(\"SD writing\")"),
            "Both status SD writing without duration"
        );
        // start_cyton_sd_write: Both → Hour24
        {
            let fn_body = src
                .split("fn start_cyton_sd_write")
                .nth(1)
                .unwrap_or("")
                .split("fn record_session_elapsed")
                .next()
                .unwrap_or("");
            assert!(
                fn_body.contains("CytonSdDuration::Hour24"),
                "Both arms Hour24 silently: {fn_body}"
            );
            assert!(
                fn_body.contains("RecordDestination::Both"),
                "Hour24 gated on Both: {fn_body}"
            );
        }
        // start_record_session must not call cyton_sd_write_start (SD on Start only).
        {
            let fn_body = src
                .split("fn start_record_session")
                .nth(1)
                .unwrap_or("")
                .split("fn stop_record_session")
                .next()
                .unwrap_or("");
            assert!(
                !fn_body.contains("start_cyton_sd_write"),
                "start_record_session must not arm SD mid-stream: {fn_body}"
            );
            assert!(
                fn_body.contains("wants_local"),
                "Record still starts local disk when dest wants it"
            );
        }
        // draw_record_export: Both Rec follows local logging only
        {
            let fn_body = src
                .split("fn draw_record_export")
                .nth(1)
                .unwrap_or("")
                .split("fn run_export_kind")
                .next()
                .unwrap_or("");
            assert!(
                fn_body.contains("data_logger.is_logging()"),
                "Both Rec follows local logging: {fn_body}"
            );
            assert!(
                fn_body.contains("format!(\"Rec {}\""),
                "Rec label beside Record/Stop Rec: {fn_body}"
            );
        }

        assert!(
            src.contains("RecordDestination::ALL"),
            "segmented Local SD Both destinations"
        );
        assert!(
            src.contains("Stop session stream also stops Record"),
            "Stop must stop Record"
        );
        assert!(
            src.contains("End session always stops Record"),
            "End must stop Record"
        );
        assert!(!src.contains("from_id_salt(\"export_kind\")"), "no standing Export format combo");
        assert!(src.contains("Play opens a recording"), "Play opens take from setup only");
    }

    #[test]
    fn experiments_picker_does_not_dump_both_scripts() {
        let src = include_str!("app.rs");
        let exp = src
            .split("\"Experiments\"")
            .nth(1)
            .unwrap_or("")
            .split("\"Networking\"")
            .next()
            .unwrap_or("");
        assert!(exp.contains("Guided recording") || exp.contains("ProtocolKind::Guided"), "{exp}");
        assert!(exp.contains("Eyes closed") || exp.contains("ProtocolKind::EyesClosed"), "{exp}");
        assert!(exp.contains("exp_protocol"));
        assert!(!exp.contains("Blink ten times"), "{exp}");
        assert!(!exp.contains("ten-minute meditation"), "{exp}");
    }
}
