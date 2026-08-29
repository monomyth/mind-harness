// OpenBCI GUI Application State
//
// Now driven by the Widget + WidgetManager system.

use crate::board::brainflow_board::BrainFlowBoard;
use crate::board::{extract_exg, recent_raw_rows, DataSource};
use crate::control_panel::{ControlPanel, DataSourceType};
use crate::data_logger::DataLogger;
use crate::event_log::EventLog;
use crate::filter_settings::NotchMode;
use crate::networking::{NetworkingManager, Protocol};
use crate::theme;
use crate::widget_context::WidgetContext;
use crate::widget_manager::WidgetManager;
use crate::widgets::{
    WAccelerometer, WBandPower, WEmg, WEmgJoystick, WFocus, WHeadPlot, WImpedance, WMarker,
    WNetworking, WSpectrogram, WTimeSeries, Widget, WFFT,
};
use directories::ProjectDirs;
use eframe::egui;
use std::collections::HashMap;
use tokio::sync::oneshot;

#[derive(Clone, Copy, PartialEq)]
pub enum SystemMode {
    PreInit,
    PostInit,
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
    udp_enabled: bool,
    udp_target: String,
    osc_enabled: bool,
    osc_target: String,

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
}

fn default_layout_id() -> usize {
    5
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
            udp_enabled: false,
            udp_target: "127.0.0.1:12345".to_string(),
            osc_enabled: false,
            osc_target: "127.0.0.1:9000".to_string(),

            // Defaults chosen to match previous hard-coded behavior + good UX
            ts_time_window_sec: 5.0,
            ts_y_scale_uv: 200.0,
            fft_smoothing_index: 2, // 0.75
            bp_smoothing_index: 2,  // 0.75
            ts_per_channel_y_scales: vec![0.0; 16],
            current_layout: 5,
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

    show_layout_customizer: bool,
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

    // Post-Phase 8 "Finish the current wave": persisted graph speed/stability settings
    persisted_ts_time_window_sec: f32,
    persisted_ts_y_scale_uv: f32,
    persisted_fft_smoothing_index: usize,
    persisted_bp_smoothing_index: usize,
    persisted_ts_per_channel_y_scales: Vec<f32>,
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
                m.insert(3, vec!["Time Series".into(), "FFT Plot".into()]);
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
            show_layout_customizer: false,
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

            // Will be overwritten by load_persisted_settings below
            persisted_ts_time_window_sec: 5.0,
            persisted_ts_y_scale_uv: 200.0,
            persisted_fft_smoothing_index: 2,
            persisted_bp_smoothing_index: 2,
            persisted_ts_per_channel_y_scales: vec![0.0; 16],
        };

        // === Phase 8: Load persisted settings (silent on any error / missing file) ===
        let persisted = OpenBciGuiApp::load_persisted_settings();
        app.control_panel.selected_source = persisted.selected_source;
        app.control_panel.synthetic_channels = persisted.synthetic_channels;
        app.control_panel.cyton_channels = persisted.cyton_channels;
        app.control_panel.playback_file = persisted.playback_file.clone();
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
                _ => {}
            }
        }
        // Phase 8: ensure persisted enabled networking targets have live senders ready for the upcoming session
        // (even though we are still in PreInit; harmless and makes first data push after Start work immediately).
        app.networking.apply_config();

        app.populate_widgets_for_new_session();
        app.populate_tool_widgets();
        app.event_log.log_system("Application started");
        app
    }

    fn set_layout(&mut self, layout: usize) {
        self.current_layout = layout;
        self.rebuild_grid_widgets_for_current_layout();
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
                _ => Box::new(WTimeSeries::new()),
            };
            wm.add_widget(widget);
        }

        wm.set_layout(self.current_layout);
        self.widget_manager = wm;
    }

    /// Create a fresh WidgetManager populated with the 4 core visualization widgets.
    /// These go into the main grid area (supports the classic layouts with up to 4 containers).
    /// Called from new() and every successful session start / end_session.
    /// Phase 7 hybrid: the interactive tool widgets (Focus, Networking, Marker, PacketLoss) are
    /// now in a dedicated SidePanel (see populate_tool_widgets) so they are never dropped.
    fn populate_widgets_for_new_session(&mut self) {
        self.emg.reset();
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

    /// Phase 7 hybrid layout (plan.md Phase 7 step 5): populate the interactive tool widgets
    /// that live in the right SidePanel. These are always visible during a session.
    /// Focus (ML + audio) is now finally usable alongside the viz — the killer Phase 6 feature
    /// is no longer hidden. Marker and Networking controls are also always at hand.
    fn populate_tool_widgets(&mut self) {
        self.tool_widgets.clear();
        self.tool_widgets.push(Box::new(WMarker::new()));
        self.tool_widgets.push(Box::new(WNetworking::new()));
        self.tool_widgets.push(Box::new(WFocus::new()));
        self.tool_widgets.push(Box::new(WHeadPlot::new()));
        self.tool_widgets.push(Box::new(WImpedance::new()));
        // NOTE: WPacketLoss is rendered inline in the SidePanel (sparkline + Reset + % ) — see the
        // "Phase 7 WPacketLoss visual" block near the tool loop. This keeps the visual right next
        // to the tools the user is looking at during an experiment; no separate widget object needed.
    }

    /// Gracefully end the current session and return to the Control Panel.
    /// This allows the user to switch boards (e.g. 8ch → 16ch Cyton) without restarting the app.
    fn end_session(&mut self) {
        // Snapshot live filter + widget state *before* dropping the board / resetting widgets.
        if let Some(ref b) = self.board {
            if let Some(settings) = b.get_filter_settings() {
                if let Some(ch) = settings.channels.first() {
                    self.last_persisted_notch_mode = NotchMode::from_channel(ch);
                    self.last_persisted_filter_bandpass = ch.bandpass_enabled;
                }
            }
        }
        self.save_current_persisted_settings();

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
            DataSourceType::CytonSerial => cp.cyton_channels,
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
                _ => {}
            }
        }

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
            udp_enabled,
            udp_target,
            osc_enabled,
            osc_target,

            ts_time_window_sec: ts_tw,
            ts_y_scale_uv: ts_ys,
            fft_smoothing_index: fft_si,
            bp_smoothing_index: bp_si,
            ts_per_channel_y_scales: ts_per_ch,
            current_layout: self.current_layout,
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
            for ch in 0..n {
                board.set_notch_filter(ch, enabled, noise);
                board.set_bandpass_filter(ch, self.last_persisted_filter_bandpass, 1.0, 50.0);
            }
            board.apply_pending_filters();
        }
    }
}

impl eframe::App for OpenBciGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count += 1;
        theme::apply_visuals(ctx);

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
                    // Phase 7 hybrid: populate viz + tool SidePanel (Focus etc. now visible)
                    self.populate_widgets_for_new_session();
                    self.populate_tool_widgets();
                    self.apply_persisted_filters_to_current_board();
                    self.save_current_persisted_settings();
                    self.system_mode = SystemMode::PostInit;
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

        if self.system_mode == SystemMode::PreInit {
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
                            // Phase 7 hybrid: populate viz grid + tool SidePanel (Focus/Networking/Marker always visible)
                            self.populate_widgets_for_new_session();
                            self.populate_tool_widgets();
                            self.apply_persisted_filters_to_current_board();
                            self.save_current_persisted_settings();
                            self.system_mode = SystemMode::PostInit;
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
                                    self.event_log.log_connection(&format!("Playback started from {}", file_path));
                                    // Phase 7 Reconnect: remember the exact playback file so "Reconnect" replays the same recording
                                    self.save_last_connection();
                                    self.populate_widgets_for_new_session();
                                    self.populate_tool_widgets();
                                    self.apply_persisted_filters_to_current_board();
                                    self.save_current_persisted_settings();
                                    self.system_mode = SystemMode::PostInit;
                                }
                                Err(e) => {
                                    self.connection_status = format!("Failed to load playback file: {}", e);
                                    self.event_log.log_error(&format!("Playback file load error: {}", e));
                                }
                            }
                        }
                        DataSourceType::SDCard => {
                            // Phase 7 stub (plan.md Phase 7 step 7)
                            self.connection_status = "SD Card: Not implemented (use Playback instead)".to_string();
                            self.event_log.log_system("SD Card selected — stub shown. Recommend Record + Playback workflow.");
                            // Do not start session; user sees message in control panel (remains visible)
                            self.control_panel.show = true;
                            return;
                        }
                    }
                }

                // Phase 7 Reconnect banner (plan.md Phase 7 polish) — always visible in PreInit when we are in Failed state.
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
                                        DataSourceType::Synthetic => format!("Synthetic ({} ch)", params.channels),
                                        DataSourceType::Playback => "the same Playback file".to_string(),
                                        _ => "last settings".to_string(),
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
                                            DataSourceType::CytonSerial => self.control_panel.cyton_channels = params.channels,
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
                                            self.populate_widgets_for_new_session();
                                            self.populate_tool_widgets();
                                            self.apply_persisted_filters_to_current_board();
                                            self.save_current_persisted_settings();
                                            self.system_mode = SystemMode::PostInit;
                                            self.connection_state = ConnectionState::Idle;
                                        } else if params.source == DataSourceType::Playback {
                                            if let Some(ref f) = params.playback_file {
                                                if let Ok(mut pb) = crate::board::playback::PlaybackBoard::from_file(std::path::Path::new(f)) {
                                                    let _ = pb.initialize();
                                                    if let Err(e) = pb.start_streaming() { tracing::error!("Reconnect pb: {}", e);} else { self.streaming = true; }
                                                    self.board = Some(Box::new(pb) as Box<dyn DataSource>);
                                                    self.connection_status = format!("Playback: {} (Reconnect)", f);
                                                    self.event_log.log_connection(&format!("Playback via Reconnect: {}", f));
                                                    self.save_last_connection();
                                                    self.populate_widgets_for_new_session();
                                                    self.populate_tool_widgets();
                                                    self.apply_persisted_filters_to_current_board();
                                                    self.save_current_persisted_settings();
                                                    self.system_mode = SystemMode::PostInit;
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
            return;
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
                                self.event_log.log_system("Impedance test started on board");
                            }
                            Err(e) => {
                                self.event_log
                                    .log_error(&format!("Impedance start failed: {e}"));
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
                                self.event_log
                                    .log_error(&format!("Impedance stop failed: {e}"));
                            }
                        }
                        imp.clear_pending();
                    }
                    if let Some(e) = b.take_impedance_error() {
                        self.event_log
                            .log_error(&format!("Impedance scan aborted: {e}"));
                    }
                }
            }
        }

        // Top bar — Java OpenBCI chrome (dark blue + light-blue subnav height ~64px)
        egui::TopBottomPanel::top("top_nav")
            .exact_height(64.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::OPENBCI_BLUE)
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading(
                    egui::RichText::new(format!("OpenBCI GUI  v{}", env!("CARGO_PKG_VERSION")))
                        .color(theme::WHITE),
                );
                ui.separator();

                let stream_label = if self.streaming {
                    "Stop Data Stream"
                } else {
                    "Start Data Stream"
                };
                let stream_fill = if self.streaming {
                    theme::TURN_OFF_RED
                } else {
                    theme::TURN_ON_GREEN
                };
                if ui
                    .add(egui::Button::new(egui::RichText::new(stream_label).color(theme::OPENBCI_DARKBLUE)).fill(stream_fill))
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
                        egui::Button::new(
                            egui::RichText::new("End Session").color(theme::WHITE),
                        )
                        .fill(theme::SUBNAV_LIGHTBLUE),
                    )
                    .clicked()
                {
                    self.end_session();
                }

                ui.separator();

                ui.label(egui::RichText::new("Layout").color(theme::WHITE));
                egui::ComboBox::from_id_salt("layout_select")
                    .selected_text(match self.current_layout {
                        1 => "1  Full",
                        2 => "2  Quad",
                        3 => "3  Split vertical",
                        4 => "4  Split horizontal",
                        5 => "5  Tall left",
                        6 => "6  Tall right",
                        _ => "Layout",
                    })
                    .show_ui(ui, |ui| {
                        let opts = [
                            (1, "1  Full"),
                            (2, "2  Quad"),
                            (3, "3  Split vertical"),
                            (4, "4  Split horizontal"),
                            (5, "5  Tall left (Java default)"),
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

                if ui.small_button("Customize").clicked() {
                    self.show_layout_customizer = true;
                }

                ui.separator();

                // Notch: Java GlobalEnvironmentalFilter (50 / 60 / 50+60 / None).
                // Shown for any live or Playback board that exposes FilterSettings.
                let mut persist_filters = false;
                if self.board.is_some() {
                    ui.label(egui::RichText::new("Notch").color(theme::WHITE));
                    let mut mode = self.last_persisted_notch_mode;
                    let mut notch_changed = false;
                    let notch_combo = egui::ComboBox::from_id_salt("notch_mode")
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for m in NotchMode::ALL {
                                if ui.selectable_value(&mut mode, m, m.label()).changed() {
                                    notch_changed = true;
                                }
                            }
                        });
                    notch_combo.response.on_hover_text(
                        "Java default is 50 + 60 Hz. A sharp FFT peak at 50 Hz with Notch 60 (or 60 Hz with Notch 50) is line noise the current notch does not remove.",
                    );
                    if notch_changed {
                        self.last_persisted_notch_mode = mode;
                        persist_filters = true;
                        self.event_log
                            .log_filter(&format!("Notch set to {}", mode.label()));
                    }

                    let mut bp = self.last_persisted_filter_bandpass;
                    if ui.checkbox(&mut bp, "BP Filt 1-50 Hz").changed() {
                        self.last_persisted_filter_bandpass = bp;
                        persist_filters = true;
                        self.event_log.log_filter(&format!(
                            "Bandpass filter {}",
                            if bp { "enabled" } else { "disabled" }
                        ));
                    }
                }
                if persist_filters {
                    self.apply_persisted_filters_to_current_board();
                    self.save_current_persisted_settings();
                }

                ui.separator();

                // Only allow channel count change for Synthetic boards (Phase 7 dyn board)
                if self
                    .board
                    .as_ref()
                    .is_some_and(|b| b.name().contains("Synthetic"))
                {
                    if ui.button("8 ch").clicked() {
                        self.switch_synthetic_channels(8);
                    }
                    if ui.button("16 ch").clicked() {
                        self.switch_synthetic_channels(16);
                    }
                }

                if let Some(ref b) = self.board {
                    let status = if self.streaming {
                        "● Streaming"
                    } else {
                        "○ Stopped"
                    };
                    ui.label(
                        egui::RichText::new(format!("{}  |  {}", b.name(), status))
                            .color(theme::WHITE),
                    );

                    ui.label(
                        egui::RichText::new(crate::stream_stats::format_hz(
                            self.current_sample_rate,
                        ))
                        .color(theme::WHITE)
                        .monospace(),
                    );
                    ui.colored_label(
                        crate::stream_stats::loss_color(self.packet_loss_percent),
                        egui::RichText::new(crate::stream_stats::format_loss(
                            self.packet_loss_percent,
                        ))
                        .monospace(),
                    );

                    if self.networking.has_active_streams() {
                        let active: Vec<&str> = self
                            .networking
                            .configs
                            .iter()
                            .filter(|c| c.enabled)
                            .map(|c| match c.protocol {
                                Protocol::UDP => "UDP",
                                Protocol::OSC => "OSC",
                                Protocol::LSL => "LSL",
                            })
                            .collect();
                        ui.colored_label(
                            egui::Color32::from_rgb(100, 180, 255),
                            format!("📡 {}", active.join("+")),
                        );
                    }

                    let mut udp_on = self
                        .networking
                        .configs
                        .iter()
                        .find(|c| c.protocol == Protocol::UDP)
                        .map(|c| c.enabled)
                        .unwrap_or(false);
                    if ui.checkbox(&mut udp_on, "UDP").changed() {
                        if let Some(cfg) = self.networking.config_mut(Protocol::UDP) {
                            cfg.enabled = udp_on;
                        }
                        self.networking.apply_config();
                        self.event_log.log_networking(&format!(
                            "UDP {}",
                            if udp_on { "enabled" } else { "disabled" }
                        ));
                    }
                    let mut osc_on = self
                        .networking
                        .configs
                        .iter()
                        .find(|c| c.protocol == Protocol::OSC)
                        .map(|c| c.enabled)
                        .unwrap_or(false);
                    if ui.checkbox(&mut osc_on, "OSC").changed() {
                        if let Some(cfg) = self.networking.config_mut(Protocol::OSC) {
                            cfg.enabled = osc_on;
                        }
                        self.networking.apply_config();
                        self.event_log.log_networking(&format!(
                            "OSC {}",
                            if osc_on { "enabled" } else { "disabled" }
                        ));
                    }

                    if ui.button("Stop All Net").clicked() {
                        self.networking.stop_all();
                        self.event_log
                            .log_networking("All networking streams stopped");
                    }
                }

                if !self.connection_status.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.connection_status).color(theme::WHITE),
                    );
                }

                if self.data_logger.is_logging() {
                    let dur = self
                        .data_logger
                        .recording_duration()
                        .map(|d| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60))
                        .unwrap_or_default();

                    ui.colored_label(
                        egui::Color32::from_rgb(220, 50, 50),
                        format!("● REC {}", dur),
                    );
                }

                ui.separator();

                // Recording format selector (only when not recording)
                if !self.data_logger.is_logging() {
                    egui::ComboBox::from_label("")
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
                                "CSV (ODF)",
                            );
                        });
                }

                let record_label = if self.data_logger.is_logging() {
                    "Stop Recording"
                } else {
                    "Record"
                };
                let record_color = if self.data_logger.is_logging() {
                    egui::Color32::from_rgb(180, 50, 50)
                } else {
                    egui::Color32::from_rgb(50, 140, 50)
                };

                if ui
                    .add(egui::Button::new(record_label).fill(record_color))
                    .clicked()
                {
                    if self.data_logger.is_logging() {
                        self.data_logger.stop();
                        self.connection_status.clear();
                        self.event_log.log_recording("Recording stopped");
                    } else {
                        let chans = if let Some(ref b) = self.board {
                            b.exg_channels().len()
                        } else {
                            8
                        };
                        let sr = if let Some(ref b) = self.board {
                            b.sample_rate()
                        } else {
                            250
                        };

                        match self.data_logger.start(self.recording_format, chans, sr) {
                            Ok(path) => {
                                self.connection_status = format!("Recording to {}", path.display());
                                self.event_log.log_recording(&format!(
                                    "Started {} recording → {}",
                                    match self.recording_format {
                                        crate::data_logger::LogFormat::BDF => "BDF",
                                        crate::data_logger::LogFormat::ODF => "ODF/CSV",
                                    },
                                    path.display()
                                ));
                            }
                            Err(e) => {
                                self.connection_status = format!("Recording failed: {}", e);
                                self.event_log
                                    .log_error(&format!("Recording start failed: {}", e));
                            }
                        }
                    }
                }
            });
        });

        // Phase 7 hybrid layout (plan.md Phase 7 step 5): right SidePanel for interactive tool widgets.
        // Marker, Networking config, Focus (ML + lock-free audio + threshold + Mark button) are
        // now *always on screen* during any session (live or Playback). This was the missing piece
        // that made the Phase 6 Focus widget invisible despite being "populated".
        // The panel is resizable, scrollable, and professional-looking with colored headers.
        // Console stays as the prominent global 📜 button + full rich window (best for audit trail).
        if !self.tool_widgets.is_empty() {
            egui::SidePanel::right("tool_panel")
                .resizable(true)
                .default_width(320.0)
                .min_width(260.0)
                .max_width(480.0)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.heading("Tools");
                        ui.small("Focus • Networking • Marker");
                        ui.separator();

                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                // Fresh ctx for tools (dropped before CentralPanel re-creates one for viz).
                                // Safe because &mut borrows to shared state (net, logger, log, last_marker) are sequential.
                                if let Some(board) = self.board.as_deref() {
                                    let mut widget_ctx = WidgetContext::new(
                                        &mut self.networking,
                                        &mut self.data_logger,
                                        &mut self.last_marker,
                                        &mut self.event_log,
                                        &mut self.emg,
                                    );

                                    for tool in &mut self.tool_widgets {
                                        // Visual card for each tool
                                        egui::Frame::NONE
                                            .fill(theme::WHITE)
                                            .stroke(egui::Stroke::new(
                                                1.0_f32,
                                                theme::OBJECT_BORDER_GREY,
                                            ))
                                            .inner_margin(6.0)
                                            .show(ui, |ui| {
                                                ui.strong(tool.title());
                                                tool.show(ui, board, &mut widget_ctx);
                                            });
                                        ui.add_space(6.0);
                                    }

                                    // Phase 7 WPacketLoss visual (sparkline + reset) — lives in SidePanel.
                                    // Uses the app's running heuristic (wall-time vs received count).
                                    // Playback always shows ~0% (perfect replay). Reset clears history + logs.
                                    let loss = self.packet_loss_percent;
                                    let loss_color = if loss > 5.0 {
                                        egui::Color32::from_rgb(255, 80, 80)
                                    } else if loss > 1.0 {
                                        egui::Color32::from_rgb(255, 200, 80)
                                    } else {
                                        egui::Color32::from_rgb(80, 200, 120)
                                    };
                                    egui::Frame::NONE
                                        .fill(theme::WHITE)
                                        .stroke(egui::Stroke::new(
                                            1.0_f32,
                                            theme::OBJECT_BORDER_GREY,
                                        ))
                                        .inner_margin(6.0)
                                        .show(ui, |ui| {
                                            ui.strong("Packet Loss");
                                            ui.horizontal(|ui| {
                                                ui.colored_label(
                                                    loss_color,
                                                    format!("{:.1}%", loss),
                                                );
                                                if ui.button("Reset").clicked() {
                                                    self.packet_loss_history.clear();
                                                    self.packet_loss_percent = 0.0;
                                                    self.window_samples = 0;
                                                    self.window_lost = 0;
                                                    self.last_sample_time = None;
                                                    self.samples_received = 0;
                                                    self.event_log.log_system(
                                                        "Packet loss stats reset by user",
                                                    );
                                                }
                                            });
                                            // Compact sparkline (last ~60 samples) — egui 0.28 compatible
                                            let hist = &self.packet_loss_history;
                                            if !hist.is_empty() {
                                                let desired =
                                                    egui::vec2(ui.available_width(), 42.0);
                                                let (resp, painter) = ui.allocate_painter(
                                                    desired,
                                                    egui::Sense::hover(),
                                                );
                                                let rect = resp.rect;
                                                let max_l = hist
                                                    .iter()
                                                    .copied()
                                                    .fold(0.0f32, |a, b| a.max(b))
                                                    .max(1.0);
                                                let n = hist.len() as f32;
                                                for (i, &v) in hist.iter().enumerate() {
                                                    let x =
                                                        rect.min.x + (i as f32 / n) * rect.width();
                                                    let y_norm = (v / max_l).min(1.0);
                                                    let y = rect.max.y - y_norm * rect.height();
                                                    if i > 0 {
                                                        let px = rect.min.x
                                                            + ((i - 1) as f32 / n) * rect.width();
                                                        let py_norm =
                                                            (hist[i - 1] / max_l).min(1.0);
                                                        let py =
                                                            rect.max.y - py_norm * rect.height();
                                                        painter.line_segment(
                                                            [egui::pos2(px, py), egui::pos2(x, y)],
                                                            egui::Stroke::new(1.5_f32, loss_color),
                                                        );
                                                    }
                                                }
                                            } else {
                                                ui.small("(no loss history yet)");
                                            }
                                        });
                                    ui.add_space(6.0);
                                }
                            });
                    });
                });
        }

        // Main widget area (the classic visualization grid — TimeSeries, FFT, BandPower, Accel)
        egui::CentralPanel::default().show(ctx, |ui| {
            // Phase 7: as_deref() yields Option<&dyn DataSource> — uniform for Playback + live boards
            if let Some(board) = self.board.as_deref() {
                self.widget_manager.update(board);

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
            }
        });

        // Bottom status / mini-console (Phase 7 — live EventLog preview + full Console window)
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Prominent "Console" button — the highest-UX win of Phase 7
                let console_btn =
                    egui::Button::new("📜 Console").fill(egui::Color32::from_rgb(70, 90, 120));
                if ui.add(console_btn).clicked() {
                    self.console_show_window = !self.console_show_window;
                }

                ui.separator();

                // Phase 7 Playback polish: compact interactive controls for the magical roundtrip.
                // Lets the user pause, change speed, and scrub the exact recording they just made
                // while Focus ML+audio, markers (sent during replay), Networking, and Console
                // continue to work exactly as in the live session. This makes validation and
                // neurofeedback rehearsal trivial without hardware.
                if let Some(b) = self.board.as_deref_mut() {
                    if let Some((pos, total)) = b.playback_progress() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
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
                                    .add(
                                        egui::Slider::new(&mut new_frac, 0.0..=1.0)
                                            .show_value(false),
                                    )
                                    .changed()
                                {
                                    b.seek_to_fraction(new_frac);
                                    self.event_log.log_system(&format!(
                                        "Playback seeked to {:.0}%",
                                        new_frac * 100.0
                                    ));
                                }
                            });
                        });
                        ui.separator();
                    }
                }

                if self.data_logger.is_logging() {
                    if let Some(path) = self.data_logger.current_file() {
                        ui.colored_label(egui::Color32::RED, "● REC");
                        ui.label(
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        );
                    }
                }

                if self.networking.has_active_streams() {
                    ui.colored_label(egui::Color32::from_rgb(100, 180, 255), "📡 Net");
                }

                if !self.last_marker.is_empty() {
                    ui.colored_label(
                        egui::Color32::LIGHT_BLUE,
                        format!("Last: {}", self.last_marker),
                    );
                }

                // Mini live log preview (last 2-3 events, color coded) — Phase 7 polish: newest first so the live tail is immediately visible
                let recent = self.event_log.last_n(3);
                if !recent.is_empty() {
                    ui.separator();
                    for entry in &recent {
                        let col = entry.level.color();
                        ui.colored_label(
                            col,
                            format!(
                                "[{}] {}",
                                &entry.category[..entry.category.len().min(4)],
                                &entry.message[..entry.message.len().min(45)]
                            ),
                        );
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(format!("Frame:{:>6}", self.frame_count));
                    ui.small(format!("Log: {} events", self.event_log.len()));
                });
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
                            content
                                .push_str("# OpenBCI GUI (Rust) — Session Event Log (visible)\n");
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
                                content.push_str("# OpenBCI GUI (Rust) — Session Event Log\n");
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

        if self.show_layout_customizer {
            let mut open = self.show_layout_customizer;
            egui::Window::new("Customize Current Layout")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    let count = WidgetManager::container_count_for(self.current_layout);
                    let assignment = self
                        .grid_layout_assignments
                        .entry(self.current_layout)
                        .or_insert_with(|| vec!["Time Series".into(); count]);
                    while assignment.len() < count {
                        assignment.push("Time Series".into());
                    }
                    assignment.truncate(count);

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
                    ];

                    for i in 0..count {
                        let mut current = assignment[i].clone();
                        egui::ComboBox::from_id_salt(format!("layout_slot_{}", i))
                            .selected_text(&current)
                            .show_ui(ui, |ui| {
                                for &opt in &options {
                                    if ui
                                        .selectable_value(&mut current, opt.to_string(), opt)
                                        .changed()
                                    {
                                        assignment[i] = current.clone();
                                        self.pending_layout_rebuild = true;
                                        self.event_log.log_system(&format!(
                                            "Layout {} position {} → {}",
                                            self.current_layout,
                                            i + 1,
                                            current
                                        ));
                                    }
                                }
                            });
                    }
                    ui.separator();
                    ui.small("Java-style containers. Changes apply immediately, per layout.");
                });
            self.show_layout_customizer = open;
        }

        if self.pending_layout_rebuild {
            self.rebuild_grid_widgets_for_current_layout();
            self.pending_layout_rebuild = false;
        }

        ctx.request_repaint();
    }
}
