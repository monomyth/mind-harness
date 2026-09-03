//! Generic BrainFlow-backed board.
//!
//! This is the Rust equivalent of the Java `BoardBrainFlow` + its subclasses.
//! It can represent Synthetic, Cyton (Serial/WiFi), Ganglion (Native/BLE/WiFi), etc.

use crate::board::ads_settings::{self, default_bank, zero_unpowered_exg, AdsChannel};
use crate::board::impedance::{
    column_window, cyton_impedance_on_cmd, ganglion_kohm, kohm_from_lead_off_std_uv,
    population_std,
};
use crate::board::ingest::ShimIngest;
use crate::board::{BoardError, DataSource};
use brainflow::{BoardIds, BrainFlowPresets, NoiseTypes};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Mutex;
#[cfg(test)]
use std::thread::ThreadId;
use std::time::{Duration, Instant};

pub struct BrainFlowBoard {
    ingest: Option<ShimIngest>,
    prepared: bool,
    board_id: BoardIds,
    serial_port: Option<String>,
    device_id: Option<String>, // for BLE / Ganglion
    ip_address: Option<String>,
    ip_port: Option<usize>,
    exg_channels: Vec<usize>,
    accel_channels: Vec<usize>,
    sample_rate: i32,
    is_streaming: bool,
    /// Unfiltered BrainFlow rows (Java `dataProcessingRawBuffer`).
    latest_data: Mutex<Vec<Vec<f64>>>,
    /// Full-window IIR of `latest_data` EXG columns (Java `dataProcessingFilteredBuffer`).
    filtered_data: Mutex<Vec<Vec<f64>>>,

    // Basic global filtering (Option A) + per-channel (Option B)
    filter_settings: crate::filter_settings::FilterSettings,

    /// Phase 7: exact count of samples delivered in the most recent board.update() / shim.get_board_data
    /// (used by app for accurate packet loss % in the WPacketLoss SidePanel visual)
    last_delivered: usize,
    last_lost: usize,
    last_ingest_error: Option<String>,
    package_num_channel: Option<usize>,
    index_tracker: Option<crate::stream_stats::SampleIndexTracker>,

    /// Impedance check currently running (UI Start/Stop).
    impedance_active: bool,
    resistance_channels: Vec<usize>,
    impedance_values: Vec<Option<f64>>,
    cyton_imp_scan: Option<CytonImpScan>,
    impedance_error: Option<String>,
    imp_io_busy: bool,
    imp_io_rx: Mutex<Option<Receiver<CytonImpIoResult>>>,
    imp_pending_kind: Option<CytonImpIoKind>,
    imp_stop_queued: bool,
    ads_bank: Vec<AdsChannel>,
    analog_channels: Vec<usize>,
    digital_channels: Vec<usize>,
    cyton_board_mode: u8,
}

#[derive(Clone, Copy)]
enum CytonImpScan {
    Measuring {
        channel: usize,
        since: Instant,
    },
    /// Stream held after `off`; wait before `on` so the board can ACK (Java 150 ms).
    OffWait {
        next: usize,
        since: Instant,
        resume_stream: bool,
    },
}

#[derive(Clone, Copy)]
enum CytonImpIoKind {
    StartOn {
        channel: usize,
    },
    SwitchOff {
        from: usize,
        next: usize,
        want_resume: bool,
    },
    SwitchOn {
        next: usize,
    },
    StopOff {
        channel: usize,
    },
    StopResume,
}

struct CytonImpIoResult {
    kind: CytonImpIoKind,
    sent: bool,
    streaming: bool,
    err: Option<String>,
}

/// Dwell long enough for a 1 s std window after ADS/lead-off settles.
const CYTON_IMP_DWELL: Duration = Duration::from_millis(2000);
const CYTON_IMP_OFF_GAP: Duration = Duration::from_millis(150);
const BRAINFLOW_STREAM_CAP: usize = 45000;

/// `config_board` on the ingest thread's live `BoardShim` only — never stop/start
/// the stream. Hardware Settings already works this way; Time Series must keep
/// filling the 1 s impedance std window. Concatenated `x…Xz…Z` is split inside
/// the worker (Cyton cannot parse both in a single `config_board`).

impl BrainFlowBoard {
    pub fn new(board_id: BoardIds, serial_port: Option<String>, device_id: Option<String>) -> Self {
        let sample_rate =
            brainflow::board_shim::get_sampling_rate(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or(250) as i32;

        let exg_channels: Vec<usize> =
            brainflow::board_shim::get_exg_channels(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or_default()
                .into_iter()
                .collect();

        let accel_channels: Vec<usize> =
            brainflow::board_shim::get_accel_channels(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or_default()
                .into_iter()
                .collect();

        let num_exg = exg_channels.len();

        let resistance_channels: Vec<usize> = brainflow::board_shim::get_resistance_channels(
            board_id,
            BrainFlowPresets::DefaultPreset,
        )
        .unwrap_or_default();

        let analog_channels: Vec<usize> =
            brainflow::board_shim::get_analog_channels(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or_default();

        let other_channels: Vec<usize> =
            brainflow::board_shim::get_other_channels(board_id, BrainFlowPresets::DefaultPreset)
                .unwrap_or_default();
        let digital_channels: Vec<usize> = other_channels
            .iter()
            .copied()
            .enumerate()
            .filter(|(i, _)| *i != 0 && *i != 5)
            .map(|(_, c)| c)
            .collect();

        let package_num_channel = brainflow::board_shim::get_package_num_channel(
            board_id,
            BrainFlowPresets::DefaultPreset,
        )
        .ok();

        let index_tracker = match board_id {
            BoardIds::CytonBoard | BoardIds::SyntheticBoard => {
                Some(crate::stream_stats::SampleIndexTracker::wrapping_0_255())
            }
            BoardIds::CytonDaisyBoard => {
                Some(crate::stream_stats::SampleIndexTracker::daisy_even_0_254())
            }
            _ => None,
        };

        Self {
            ingest: None,
            prepared: false,
            board_id,
            serial_port,
            device_id,
            ip_address: None,
            ip_port: None,
            exg_channels,
            accel_channels,
            sample_rate,
            is_streaming: false,
            latest_data: Mutex::new(Vec::new()),
            filtered_data: Mutex::new(Vec::new()),

            // Per-channel filter settings (Option B foundation)
            filter_settings: crate::filter_settings::FilterSettings::new(num_exg),

            last_delivered: 0,
            last_lost: 0,
            last_ingest_error: None,
            package_num_channel,
            index_tracker,
            impedance_active: false,
            resistance_channels,
            impedance_values: vec![None; num_exg],
            cyton_imp_scan: None,
            impedance_error: None,
            imp_io_busy: false,
            imp_io_rx: Mutex::new(None),
            imp_pending_kind: None,
            imp_stop_queued: false,
            ads_bank: default_bank(num_exg),
            analog_channels,
            digital_channels,
            cyton_board_mode: 0,
        }
    }

    /// Convenience constructor for Synthetic board.
    /// Keep BrainFlow's real EXG indices (never assume column 0 is EEG — it is often the
    /// sample index). Truncate to the user-requested channel count.
    pub fn synthetic(num_channels: usize) -> Self {
        let mut board = Self::new(BoardIds::SyntheticBoard, None, None);
        let n = num_channels.max(1);
        if board.exg_channels.is_empty() {
            board.exg_channels = (0..n).collect();
        } else {
            board.exg_channels.truncate(n);
        }
        let n = board.exg_channels.len();
        board.filter_settings = crate::filter_settings::FilterSettings::new(n);
        board.impedance_values = vec![None; n];
        board.ads_bank = default_bank(n);
        board
    }

    /// Convenience for Cyton 8-channel via serial
    pub fn cyton_serial(port: &str) -> Self {
        Self::new(BoardIds::CytonBoard, Some(port.to_string()), None)
    }

    /// Convenience for Cyton + Daisy (16-channel) via serial
    pub fn cyton_serial_daisy(port: &str) -> Self {
        Self::new(BoardIds::CytonDaisyBoard, Some(port.to_string()), None)
    }

    /// Ganglion via BrainFlow native BLE (MAC or advertised name).
    pub fn ganglion_native(device_id: &str) -> Self {
        Self::new(
            BoardIds::GanglionNativeBoard,
            None,
            Some(device_id.to_string()),
        )
    }

    /// Cyton WiFi shield (BrainFlow `CYTON_WIFI_BOARD`, IP + port 6677).
    pub fn cyton_wifi(ip: &str, daisy: bool) -> Self {
        let id = if daisy {
            BoardIds::CytonDaisyWifiBoard
        } else {
            BoardIds::CytonWifiBoard
        };
        let mut board = Self::new(id, None, None);
        board.ip_address = Some(ip.trim().to_string());
        board.ip_port = Some(6677);
        board
    }

    #[allow(dead_code)]
    pub fn wifi_endpoint(&self) -> Option<(String, usize)> {
        Some((self.ip_address.clone()?, self.ip_port.unwrap_or(6677)))
    }
}

impl DataSource for BrainFlowBoard {
    fn initialize(&mut self) -> Result<(), BoardError> {
        if self.prepared {
            return Ok(());
        }
        if self.ingest.is_none() {
            self.ingest = Some(ShimIngest::spawn());
        }
        let ingest = self.ingest.as_ref().expect("ingest spawned");
        ingest.prepare(
            self.board_id,
            self.serial_port.clone(),
            self.device_id.clone(),
            self.ip_address.clone(),
            self.ip_port,
        )?;
        self.prepared = true;
        self.last_delivered = 0;
        self.clear_buffers();
        Ok(())
    }

    fn uninitialize(&mut self) -> Result<(), BoardError> {
        self.drain_cyton_imp_io();
        if self.impedance_active && self.is_ads1299() {
            // Session teardown can hitch; restore ADS before releasing the port.
            if let Some(ch) = self.impedance_scan_off_channel() {
                if let Some(cmd) = self.ads_restore_imp_cmd(ch) {
                    if let Some(ingest) = self.ingest.as_ref() {
                        let _ = ingest.config(&cmd);
                    }
                }
            }
        }
        if let Some(ingest) = self.ingest.take() {
            ingest.shutdown();
        }
        self.prepared = false;
        self.is_streaming = false;
        self.last_delivered = 0;
        self.last_lost = 0;
        if let Some(t) = self.index_tracker.as_mut() {
            t.reset();
        }
        self.impedance_active = false;
        self.cyton_imp_scan = None;
        self.impedance_error = None;
        self.imp_io_busy = false;
        self.imp_stop_queued = false;
        self.imp_pending_kind = None;
        self.impedance_values.fill(None);
        self.clear_buffers();
        Ok(())
    }

    fn update(&mut self) {
        self.last_delivered = 0;
        self.last_lost = 0;
        self.poll_cyton_imp_io();
        if self.imp_io_busy {
            return;
        }
        if self.impedance_active && self.is_ads1299() && !self.is_streaming {
            self.tick_cyton_impedance();
            return;
        }
        self.last_ingest_error = None;
        let (mut new_samples, err) = match self.ingest.as_ref() {
            Some(ingest) if self.is_streaming => ingest.take_rows(),
            _ => (Vec::new(), None),
        };
        self.last_ingest_error = err;
        if new_samples.is_empty() {
            if self.impedance_active && self.is_ads1299() {
                self.tick_cyton_impedance();
            }
            return;
        }

        // Rows are already sample-major from the ingest thread. Do **not** IIR
        // on a short BrainFlow slice: a cold Butterworth destroys blinks / slow EEG.
        for row in &mut new_samples {
            zero_unpowered_exg(row, &self.exg_channels, &self.ads_bank);
        }

        let delivered = new_samples.len();
        if let (Some(pkg), Some(tracker)) = (self.package_num_channel, self.index_tracker.as_mut())
        {
            let mut lost = 0u64;
            for row in &new_samples {
                if let Some(&v) = row.get(pkg) {
                    lost += tracker.observe(v as i32);
                }
            }
            self.last_lost = lost as usize;
        }
        if self.impedance_active && self.is_ganglion() {
            self.ingest_ganglion_resistance(&new_samples);
        }
        let max_keep = crate::filter_settings::display_buffer_keep(self.sample_rate as usize);
        let filtered = if let Ok(mut guard) = self.latest_data.lock() {
            crate::filter_settings::append_raw_and_filter(
                &mut guard,
                new_samples,
                max_keep,
                &self.exg_channels,
                self.sample_rate as usize,
                &self.filter_settings,
            )
        } else {
            Vec::new()
        };
        if let Ok(mut fg) = self.filtered_data.lock() {
            *fg = filtered;
        }
        self.last_delivered = delivered;
        if self.impedance_active && self.is_ads1299() {
            self.tick_cyton_impedance();
        }
    }

    fn start_streaming(&mut self) -> Result<(), BoardError> {
        if self.imp_io_busy {
            return Err(BoardError::Io("board busy with impedance command".into()));
        }
        let Some(ingest) = self.ingest.as_ref() else {
            return Err(BoardError::NotInitialized);
        };
        if !self.is_streaming {
            ingest.start_stream(BRAINFLOW_STREAM_CAP)?;
            self.is_streaming = true;
            self.last_delivered = 0;
            self.last_lost = 0;
            if let Some(t) = self.index_tracker.as_mut() {
                t.reset();
            }
            self.clear_buffers();
        }
        Ok(())
    }

    fn stop_streaming(&mut self) -> Result<(), BoardError> {
        if self.imp_io_busy {
            return Err(BoardError::Io("board busy with impedance command".into()));
        }
        if let Some(ingest) = self.ingest.as_ref() {
            if self.is_streaming {
                ingest.stop_stream()?;
                self.is_streaming = false;
            }
        }
        Ok(())
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn total_channel_count(&self) -> usize {
        // Approximate – real implementation should query board descriptor
        self.exg_channels.len() + 4
    }

    fn exg_channels(&self) -> &[usize] {
        &self.exg_channels
    }

    fn accel_channels(&self) -> &[usize] {
        &self.accel_channels
    }

    fn package_num_channel(&self) -> Option<usize> {
        self.package_num_channel
    }

    fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    fn get_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        tail_locked(&self.filtered_data, max_samples)
    }

    fn get_raw_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        tail_locked(&self.latest_data, max_samples)
    }

    fn get_frame_data(&self) -> Vec<Vec<f64>> {
        if let Ok(guard) = self.filtered_data.lock() {
            guard.last().cloned().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    fn name(&self) -> &str {
        match self.board_id {
            BoardIds::SyntheticBoard => "BrainFlow Synthetic",
            BoardIds::CytonBoard => "Cyton (8ch)",
            BoardIds::CytonDaisyBoard => "Cyton + Daisy (16ch)",
            BoardIds::GanglionNativeBoard => "Ganglion (Native BLE)",
            BoardIds::CytonWifiBoard => "Cyton WiFi (8ch)",
            BoardIds::CytonDaisyWifiBoard => "Cyton WiFi + Daisy (16ch)",
            _ => "BrainFlow Board",
        }
    }

    // Phase 7: provide filter settings via the DataSource trait so it works
    // uniformly whether the board is BrainFlowBoard or PlaybackBoard (latter returns None).
    fn get_filter_settings(&self) -> Option<&crate::filter_settings::FilterSettings> {
        Some(&self.filter_settings)
    }

    fn set_notch_filter(&mut self, channel: usize, enabled: bool, noise_type: NoiseTypes) {
        self.filter_settings.set_notch(channel, enabled, noise_type);
    }

    fn set_bandpass_filter(&mut self, channel: usize, enabled: bool, low: f64, high: f64) {
        self.filter_settings
            .set_bandpass(channel, enabled, low, high);
    }

    fn apply_pending_filters(&mut self) {
        self.rebuild_filtered();
    }

    fn recent_samples_delivered(&self) -> usize {
        self.last_delivered
    }

    fn recent_samples_lost(&self) -> usize {
        self.last_lost
    }

    fn last_ingest_error(&self) -> Option<String> {
        self.last_ingest_error.clone()
    }

    fn supports_impedance(&self) -> bool {
        self.is_ads1299() || self.is_ganglion() || self.board_id == BoardIds::SyntheticBoard
    }

    fn start_impedance_test(&mut self, _channels: &[usize]) -> Result<(), BoardError> {
        self.poll_cyton_imp_io();
        let n = self.exg_channels.len();
        self.impedance_values = vec![None; n];
        self.cyton_imp_scan = None;
        self.imp_stop_queued = false;
        self.impedance_error = None;

        if self.board_id == BoardIds::SyntheticBoard {
            self.impedance_active = true;
            return Ok(());
        }

        if !self.prepared {
            return Err(BoardError::NotInitialized);
        }

        if self.is_ganglion() {
            self.config_board_str("z")?;
            self.impedance_active = true;
            return Ok(());
        }

        if self.is_ads1299() {
            let cmd = cyton_impedance_on_cmd(0)
                .ok_or_else(|| BoardError::Io("Cyton has no EXG channels for impedance".into()))?;
            // Keep Measuring before ACK so Stop can restore if resume fails after `on`.
            self.impedance_active = true;
            self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                channel: 0,
                since: Instant::now(),
            });
            if let Err(e) = self.launch_cyton_io(Some(cmd), CytonImpIoKind::StartOn { channel: 0 })
            {
                self.impedance_active = false;
                self.cyton_imp_scan = None;
                return Err(e);
            }
            return Ok(());
        }

        Err(BoardError::Io(
            "impedance is not supported on this board".into(),
        ))
    }

    fn stop_impedance_test(&mut self) -> Result<(), BoardError> {
        self.poll_cyton_imp_io();
        if self.imp_io_busy {
            self.imp_stop_queued = true;
            return Ok(());
        }
        self.begin_cyton_imp_stop()
    }

    fn get_impedance(&self) -> Vec<Option<f64>> {
        let n = self.exg_channels.len();
        if !self.impedance_active {
            return vec![None; n];
        }
        // Only Synthetic may invent numbers; the UI labels them simulated.
        if self.board_id == BoardIds::SyntheticBoard {
            return (0..n).map(|i| Some(5.0 + (i as f64) * 0.8)).collect();
        }
        let mut out = vec![None; n];
        for (i, v) in self.impedance_values.iter().take(n).enumerate() {
            out[i] = v.filter(|k| *k > 0.0);
        }
        out
    }

    fn impedance_is_simulated(&self) -> bool {
        self.board_id == BoardIds::SyntheticBoard
    }

    fn impedance_quality_kohm(&self) -> (f64, f64) {
        if self.board_id == BoardIds::SyntheticBoard {
            (5.0, 15.0)
        } else if self.is_ads1299() {
            (750.0, 2500.0)
        } else if self.is_ganglion() {
            (50.0, 150.0)
        } else {
            (5.0, 15.0)
        }
    }

    fn impedance_scan_channel(&self) -> Option<usize> {
        if !self.impedance_active {
            return None;
        }
        match self.cyton_imp_scan {
            Some(CytonImpScan::Measuring { channel, .. }) => Some(channel),
            Some(CytonImpScan::OffWait { next, .. }) => Some(next),
            None => None,
        }
    }

    fn impedance_test_active(&self) -> bool {
        self.impedance_active
    }

    fn take_impedance_error(&mut self) -> Option<String> {
        self.impedance_error.take()
    }

    fn ads_channels(&self) -> Option<&[AdsChannel]> {
        if self.is_ads1299()
            || self.board_id == BoardIds::SyntheticBoard
            || matches!(
                self.board_id,
                BoardIds::CytonWifiBoard | BoardIds::CytonDaisyWifiBoard
            )
        {
            Some(&self.ads_bank)
        } else {
            None
        }
    }

    fn commit_ads_channel(
        &mut self,
        channel: usize,
        settings: AdsChannel,
    ) -> Result<(), BoardError> {
        if channel >= self.ads_bank.len() {
            return Err(BoardError::Io("channel out of range".into()));
        }
        let cmd = ads_settings::ads_commit_cmd(channel, settings)
            .ok_or_else(|| BoardError::Io("no ADS letter for channel".into()))?;
        if self.board_id == BoardIds::SyntheticBoard || !self.prepared {
            self.ads_bank[channel] = settings;
            return Ok(());
        }
        if !self.is_ads1299()
            && !matches!(
                self.board_id,
                BoardIds::CytonWifiBoard | BoardIds::CytonDaisyWifiBoard
            )
        {
            return Err(BoardError::Io(
                "hardware settings are Cyton ADS1299 only".into(),
            ));
        }
        self.config_board_str(&cmd)?;
        self.ads_bank[channel] = settings;
        Ok(())
    }

    fn channel_powered(&self) -> Vec<bool> {
        self.ads_bank
            .iter()
            .map(|s| s.power == ads_settings::AdsPower::On)
            .collect()
    }

    fn analog_channels(&self) -> &[usize] {
        &self.analog_channels
    }

    fn digital_channels(&self) -> &[usize] {
        &self.digital_channels
    }

    fn cyton_board_mode(&self) -> Option<u8> {
        if self.supports_aux_widgets() {
            Some(self.cyton_board_mode)
        } else {
            None
        }
    }

    fn set_cyton_board_mode(&mut self, mode: u8) -> Result<(), BoardError> {
        if !self.supports_aux_widgets() {
            return Err(BoardError::Io("aux mode is Cyton-only".into()));
        }
        if self.prepared {
            self.config_board_str(&format!("/{mode}"))?;
        }
        self.cyton_board_mode = mode;
        Ok(())
    }

    fn supports_aux_widgets(&self) -> bool {
        matches!(
            self.board_id,
            BoardIds::CytonBoard
                | BoardIds::CytonDaisyBoard
                | BoardIds::CytonWifiBoard
                | BoardIds::CytonDaisyWifiBoard
        )
    }
}

fn tail_locked(buf: &Mutex<Vec<Vec<f64>>>, max_samples: usize) -> Vec<Vec<f64>> {
    if let Ok(guard) = buf.lock() {
        let len = guard.len();
        let start = len.saturating_sub(max_samples);
        guard[start..].to_vec()
    } else {
        Vec::new()
    }
}

// Inherent methods for BrainFlowBoard (filter controls + internal use)
// Note: set_* are also provided via DataSource trait override so they work on Box<dyn DataSource>
impl BrainFlowBoard {
    fn is_ads1299(&self) -> bool {
        matches!(
            self.board_id,
            BoardIds::CytonBoard
                | BoardIds::CytonDaisyBoard
                | BoardIds::CytonWifiBoard
                | BoardIds::CytonDaisyWifiBoard
        )
    }

    fn ads_restore_imp_cmd(&self, channel: usize) -> Option<String> {
        let ads = self.ads_bank.get(channel).copied().unwrap_or_default();
        ads_settings::ads_impedance_restore_cmd(channel, ads)
    }

    fn is_ganglion(&self) -> bool {
        matches!(self.board_id, BoardIds::GanglionNativeBoard)
    }

    fn config_board_str(&self, cmd: &str) -> Result<(), BoardError> {
        let ingest = self.ingest.as_ref().ok_or(BoardError::NotInitialized)?;
        ingest.config(cmd)
    }

    fn impedance_scan_off_channel(&self) -> Option<usize> {
        match self.cyton_imp_scan {
            Some(CytonImpScan::Measuring { channel, .. }) => Some(channel),
            Some(CytonImpScan::OffWait { .. }) => None,
            None => None,
        }
    }

    fn launch_cyton_io(
        &mut self,
        cmd: Option<String>,
        kind: CytonImpIoKind,
    ) -> Result<(), BoardError> {
        if self.imp_io_busy {
            return Err(BoardError::Io(
                "impedance command already in progress".into(),
            ));
        }
        if self.ingest.is_none() {
            return Err(BoardError::NotInitialized);
        };
        let (sent, streaming, err) = match cmd.as_deref() {
            None => (true, self.is_streaming, None),
            Some(cmd) => match self.config_board_str(cmd) {
                Ok(()) => (true, self.is_streaming, None),
                Err(e) => (false, self.is_streaming, Some(e.to_string())),
            },
        };
        let io_err = err.clone();
        self.apply_cyton_imp_io_result(CytonImpIoResult {
            kind,
            sent,
            streaming,
            err,
        });
        if self.imp_stop_queued && self.impedance_active {
            self.imp_stop_queued = false;
            let _ = self.begin_cyton_imp_stop();
        }
        if let Some(e) = io_err {
            return Err(BoardError::BrainFlow(e));
        }
        Ok(())
    }

    fn drain_cyton_imp_io(&mut self) {
        let rx = self.imp_io_rx.lock().ok().and_then(|mut g| g.take());
        if let Some(rx) = rx {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
        self.imp_io_busy = false;
        self.imp_pending_kind = None;
    }

    fn poll_cyton_imp_io(&mut self) {
        let result = {
            let Ok(mut slot) = self.imp_io_rx.lock() else {
                return;
            };
            let Some(rx) = slot.as_mut() else {
                return;
            };
            match rx.try_recv() {
                Ok(r) => {
                    *slot = None;
                    Some(r)
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    *slot = None;
                    let kind = self.imp_pending_kind.unwrap_or(CytonImpIoKind::StopResume);
                    Some(CytonImpIoResult {
                        kind,
                        sent: false,
                        streaming: self.is_streaming,
                        err: Some("impedance worker disconnected".into()),
                    })
                }
            }
        };
        let Some(result) = result else {
            return;
        };
        self.imp_io_busy = false;
        self.imp_pending_kind = None;
        self.apply_cyton_imp_io_result(result);
        if self.imp_stop_queued && !self.imp_io_busy && self.impedance_active {
            self.imp_stop_queued = false;
            let _ = self.begin_cyton_imp_stop();
        }
    }

    fn apply_cyton_imp_io_result(&mut self, result: CytonImpIoResult) {
        let was_streaming = self.is_streaming;
        self.is_streaming = result.streaming;
        if result.streaming && !was_streaming {
            self.last_delivered = 0;
            self.last_lost = 0;
            if let Some(t) = self.index_tracker.as_mut() {
                t.reset();
            }
            self.clear_buffers();
        }
        if let Some(e) = result.err {
            tracing::warn!("Cyton impedance IO: {e}");
            self.impedance_error = Some(e);
        }
        match result.kind {
            CytonImpIoKind::StartOn { channel } => {
                if result.sent {
                    self.impedance_active = true;
                    self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                        channel,
                        since: Instant::now(),
                    });
                } else {
                    self.impedance_active = false;
                    self.cyton_imp_scan = None;
                }
            }
            CytonImpIoKind::SwitchOff {
                from,
                next,
                want_resume,
            } => {
                if result.sent {
                    self.cyton_imp_scan = Some(CytonImpScan::OffWait {
                        next,
                        since: Instant::now(),
                        resume_stream: want_resume,
                    });
                } else {
                    self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                        channel: from,
                        since: Instant::now(),
                    });
                }
            }
            CytonImpIoKind::SwitchOn { next } => {
                if result.sent {
                    self.impedance_active = true;
                    self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                        channel: next,
                        since: Instant::now(),
                    });
                } else {
                    self.cyton_imp_scan = None;
                    self.impedance_active = false;
                }
            }
            CytonImpIoKind::StopOff { channel } => {
                if result.sent {
                    self.cyton_imp_scan = None;
                    self.impedance_active = false;
                    self.impedance_values.fill(None);
                } else {
                    self.impedance_active = true;
                    self.cyton_imp_scan = Some(CytonImpScan::Measuring {
                        channel,
                        since: Instant::now(),
                    });
                }
            }
            CytonImpIoKind::StopResume => {
                self.cyton_imp_scan = None;
                self.impedance_active = false;
                self.impedance_values.fill(None);
            }
        }
    }

    fn begin_cyton_imp_stop(&mut self) -> Result<(), BoardError> {
        if self.is_ganglion() && self.impedance_active {
            self.config_board_str("Z")?;
            self.impedance_active = false;
            self.impedance_values.fill(None);
            return Ok(());
        }
        match self.cyton_imp_scan {
            Some(CytonImpScan::Measuring { channel, .. }) => {
                let cmd = self.ads_restore_imp_cmd(channel).ok_or_else(|| {
                    BoardError::Io("Cyton has no EXG channels for impedance".into())
                })?;
                self.launch_cyton_io(Some(cmd), CytonImpIoKind::StopOff { channel })
            }
            Some(CytonImpScan::OffWait { .. }) => {
                self.launch_cyton_io(None, CytonImpIoKind::StopResume)
            }
            None => {
                self.impedance_active = false;
                self.impedance_values.fill(None);
                Ok(())
            }
        }
    }

    fn ingest_ganglion_resistance(&mut self, rows: &[Vec<f64>]) {
        let n = self.exg_channels.len();
        if self.impedance_values.len() != n {
            self.impedance_values.resize(n, None);
        }
        for row in rows {
            for (i, &col) in self.resistance_channels.iter().take(n).enumerate() {
                if let Some(&v) = row.get(col) {
                    if let Some(kohm) = ganglion_kohm(v) {
                        self.impedance_values[i] = Some(kohm);
                    }
                }
            }
        }
    }

    fn tick_cyton_impedance(&mut self) {
        if self.imp_io_busy {
            return;
        }
        let n = self.exg_channels.len();
        if n == 0 {
            return;
        }
        match self.cyton_imp_scan {
            Some(CytonImpScan::Measuring { channel, since })
                if since.elapsed() >= CYTON_IMP_DWELL =>
            {
                if let Some(kohm) = self.cyton_kohm_for_channel(channel) {
                    if channel < self.impedance_values.len() {
                        self.impedance_values[channel] = Some(kohm);
                    }
                }
                let next = (channel + 1) % n;
                let Some(off) = self.ads_restore_imp_cmd(channel) else {
                    return;
                };
                let want_resume = self.is_streaming;
                if let Err(e) = self.launch_cyton_io(
                    Some(off),
                    CytonImpIoKind::SwitchOff {
                        from: channel,
                        next,
                        want_resume,
                    },
                ) {
                    self.impedance_error = Some(e.to_string());
                }
            }
            Some(CytonImpScan::OffWait {
                next,
                since,
                resume_stream,
            }) if since.elapsed() >= CYTON_IMP_OFF_GAP => {
                let Some(on) = cyton_impedance_on_cmd(next) else {
                    self.impedance_error = Some("Cyton has no EXG channels for impedance".into());
                    self.cyton_imp_scan = None;
                    self.impedance_active = false;
                    if resume_stream {
                        let _ = self.launch_cyton_io(None, CytonImpIoKind::StopResume);
                    }
                    return;
                };
                if let Err(e) = self.launch_cyton_io(Some(on), CytonImpIoKind::SwitchOn { next }) {
                    self.impedance_error = Some(e.to_string());
                }
            }
            _ => {}
        }
    }

    fn cyton_kohm_for_channel(&self, ch: usize) -> Option<f64> {
        let col = *self.exg_channels.get(ch)?;
        let window = self.sample_rate.max(1) as usize;
        let guard = self.latest_data.lock().ok()?;
        let xs = column_window(&guard, col, window)?;
        let std = population_std(&xs)?;
        Some(kohm_from_lead_off_std_uv(std))
    }

    fn clear_buffers(&mut self) {
        if let Ok(mut g) = self.latest_data.lock() {
            g.clear();
        }
        if let Ok(mut g) = self.filtered_data.lock() {
            g.clear();
        }
    }

    #[cfg(test)]
    pub(crate) fn last_shim_pull_thread(&self) -> Option<ThreadId> {
        self.ingest.as_ref().and_then(|i| i.last_pull_thread())
    }

    #[cfg(test)]
    pub(crate) fn ingest_cmds_sent(&self) -> u64 {
        self.ingest.as_ref().map(|i| i.cmds_sent()).unwrap_or(0)
    }

    fn rebuild_filtered(&mut self) {
        let filtered = if let Ok(guard) = self.latest_data.lock() {
            crate::filter_settings::rebuild_filtered_display(
                &guard,
                &self.exg_channels,
                self.sample_rate as usize,
                &self.filter_settings,
            )
        } else {
            return;
        };
        if let Ok(mut fg) = self.filtered_data.lock() {
            *fg = filtered;
        }
    }

    /// Phase 7: direct accessors for filter state (Option B). Currently the widgets and app
    /// go through the DataSource trait (which delegates), so these are not called directly.
    /// Retained for future widget or inspector use; allowed to keep build clean.
    #[allow(dead_code)]
    pub fn get_filter_settings(&self) -> &crate::filter_settings::FilterSettings {
        &self.filter_settings
    }

    #[allow(dead_code)]
    pub fn get_filter_settings_mut(&mut self) -> &mut crate::filter_settings::FilterSettings {
        &mut self.filter_settings
    }
}

impl Drop for BrainFlowBoard {
    fn drop(&mut self) {
        if self.ingest.is_some() {
            let _ = self.uninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::DataSource;

    #[test]
    fn synthetic_exg_skips_package_num_column() {
        let b = BrainFlowBoard::synthetic(8);
        assert_eq!(
            b.exg_channels(),
            &[1, 2, 3, 4, 5, 6, 7, 8],
            "electrode 1 must be BrainFlow column 1, not package_num 0"
        );
    }

    #[test]
    fn cyton_exg_skips_package_num_column() {
        let b = BrainFlowBoard::cyton_serial("/dev/null");
        assert_eq!(b.exg_channels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn ganglion_exg_skips_package_num_column() {
        let b = BrainFlowBoard::ganglion_native("test");
        assert_eq!(b.exg_channels(), &[1, 2, 3, 4]);
    }

    #[test]
    fn cyton_impedance_is_live_not_simulated() {
        let mut b = BrainFlowBoard::cyton_serial("/dev/null");
        assert!(b.supports_impedance());
        assert!(!b.impedance_is_simulated());
        assert_eq!(b.impedance_quality_kohm(), (750.0, 2500.0));
        assert_eq!(b.get_impedance(), vec![None; 8]);
        assert!(!b.prepared);
        let err = b.start_impedance_test(&[0]).unwrap_err();
        assert!(
            matches!(err, BoardError::NotInitialized),
            "live kOhm must not open a second serial session: {err}"
        );
        assert!(!b.impedance_test_active());
        assert!(!b.impedance_is_simulated());
        assert_eq!(b.get_impedance(), vec![None; 8]);
        assert!(b.take_impedance_error().is_none());
    }

    #[test]
    fn synthetic_impedance_is_labelled_simulated() {
        let mut b = BrainFlowBoard::synthetic(8);
        assert!(b.supports_impedance());
        assert!(b.impedance_is_simulated());
        assert_eq!(b.impedance_quality_kohm(), (5.0, 15.0));
        assert_eq!(b.get_impedance(), vec![None; 8]);
        b.start_impedance_test(&[0, 1]).unwrap();
        assert!(b.impedance_test_active());
        let vals = b.get_impedance();
        assert_eq!(vals.len(), 8);
        assert!(vals.iter().all(|v| v.is_some()));
        b.stop_impedance_test().unwrap();
        assert!(!b.impedance_test_active());
        assert!(b.get_impedance().iter().all(|v| v.is_none()));
    }

    #[test]
    fn ganglion_impedance_is_live_not_simulated() {
        let mut b = BrainFlowBoard::ganglion_native("test");
        assert!(b.supports_impedance());
        assert!(!b.impedance_is_simulated());
        assert_eq!(b.impedance_quality_kohm(), (50.0, 150.0));
        assert_eq!(b.get_impedance(), vec![None; 4]);
        assert!(b.start_impedance_test(&[0]).is_err());
        assert!(!b.impedance_test_active());
        assert_eq!(b.get_impedance(), vec![None; 4]);
    }

    #[test]
    fn cyton_on_sent_keeps_scan_if_resume_fails() {
        let mut b = BrainFlowBoard::cyton_serial("/dev/null");
        b.apply_cyton_imp_io_result(CytonImpIoResult {
            kind: CytonImpIoKind::StartOn { channel: 0 },
            sent: true,
            streaming: false,
            err: Some("start_stream failed".into()),
        });
        assert!(b.impedance_test_active());
        assert_eq!(b.impedance_scan_channel(), Some(0));
        assert!(!b.is_streaming());
        assert!(b.take_impedance_error().is_some());
    }

    #[test]
    fn cyton_switch_on_sent_keeps_next_channel_if_resume_fails() {
        let mut b = BrainFlowBoard::cyton_serial("/dev/null");
        b.apply_cyton_imp_io_result(CytonImpIoResult {
            kind: CytonImpIoKind::SwitchOn { next: 3 },
            sent: true,
            streaming: false,
            err: Some("start_stream failed".into()),
        });
        assert!(b.impedance_test_active());
        assert_eq!(b.impedance_scan_channel(), Some(3));
    }

    #[test]
    fn cyton_off_not_sent_keeps_previous_channel() {
        let mut b = BrainFlowBoard::cyton_serial("/dev/null");
        b.impedance_active = true;
        b.apply_cyton_imp_io_result(CytonImpIoResult {
            kind: CytonImpIoKind::SwitchOff {
                from: 2,
                next: 3,
                want_resume: true,
            },
            sent: false,
            streaming: true,
            err: Some("config_board failed".into()),
        });
        assert_eq!(b.impedance_scan_channel(), Some(2));
    }

    #[test]
    fn cyton_stop_off_not_sent_keeps_measuring() {
        let mut b = BrainFlowBoard::cyton_serial("/dev/null");
        b.apply_cyton_imp_io_result(CytonImpIoResult {
            kind: CytonImpIoKind::StopOff { channel: 1 },
            sent: false,
            streaming: false,
            err: Some("off failed".into()),
        });
        assert!(b.impedance_test_active());
        assert_eq!(b.impedance_scan_channel(), Some(1));
    }

    #[test]
    fn synthetic_shim_pull_is_not_on_update_thread() {
        let mut b = BrainFlowBoard::synthetic(8);
        b.initialize().expect("synthetic initialize");
        b.start_streaming().expect("synthetic start");
        std::thread::sleep(Duration::from_millis(80));
        b.update();
        let pull_tid = b.last_shim_pull_thread();
        let delivered = b.recent_samples_delivered();
        let _ = b.stop_streaming();
        let _ = b.uninitialize();
        assert!(
            pull_tid.is_some(),
            "ingest thread must have called get_board_data"
        );
        assert_ne!(
            pull_tid,
            Some(std::thread::current().id()),
            "get_board_data must not run on the UI/update thread (live Cyton dies after the first burst when it does)"
        );
        assert!(
            delivered > 0,
            "80ms of synthetic ingest must deliver samples to update(), got {delivered}"
        );
    }

    #[test]
    fn synthetic_power_off_channel_8_zeros_exg() {
        let mut b = BrainFlowBoard::synthetic(8);
        let off = AdsChannel {
            power: ads_settings::AdsPower::Off,
            ..AdsChannel::default()
        };
        b.commit_ads_channel(7, off).unwrap();
        assert!(!b.channel_powered()[7]);
        let mut row = vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        zero_unpowered_exg(&mut row, b.exg_channels(), b.ads_channels().unwrap());
        assert_eq!(row[8], 0.0);
        assert_eq!(row[1], 1.0);
    }

    #[test]
    fn cyton_wifi_config_has_ip_and_port() {
        let b = BrainFlowBoard::cyton_wifi("192.168.4.1", false);
        assert_eq!(b.wifi_endpoint(), Some(("192.168.4.1".into(), 6677)));
        assert!(!b.supports_aux_widgets() || b.analog_channels().len() <= 8);
        assert!(b.supports_aux_widgets());
        assert!(!b.impedance_is_simulated());
    }

    #[test]
    fn synthetic_hides_aux_widgets() {
        let b = BrainFlowBoard::synthetic(8);
        assert!(!b.supports_aux_widgets());
        assert!(b.ads_channels().is_some());
    }

    #[test]
    fn record_start_does_not_call_board_shim_or_stop_ingest() {
        let mut b = BrainFlowBoard::synthetic(8);
        b.initialize().expect("init");
        b.start_streaming().expect("start");
        std::thread::sleep(Duration::from_millis(50));
        b.update();
        assert!(
            b.recent_samples_delivered() > 0,
            "synthetic ingest must be alive before Record"
        );
        let cmds = b.ingest_cmds_sent();
        let mut pump = crate::data_logger::RecordPump::spawn();
        let path = pump
            .start(crate::data_logger::LogFormat::BDF, 8, 250)
            .expect("record");
        assert_eq!(
            b.ingest_cmds_sent(),
            cmds,
            "Record start must not call BoardShim"
        );
        std::thread::sleep(Duration::from_millis(50));
        b.update();
        assert!(
            b.recent_samples_delivered() > 0,
            "ingest must keep delivering after Record start"
        );
        pump.stop();
        let _ = b.stop_streaming();
        let _ = b.uninitialize();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(crate::markers::sidecar_path(&path));
    }
}
