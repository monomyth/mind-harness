//! Control Panel (PreInit state) — significantly improved.
//!
//! Supports Synthetic + real serial port discovery for Cyton-style boards.

// Serial port discovery & selection strategy (Phase 8 cross-platform polish):
// - macOS: cu.* (callout) ports are preferred over tty.* for USB-serial dongles.
//   All cu/ tty helpers, sort logic, warning labels and "Use cu instead" buttons are
//   gated with #[cfg(target_os = "macos")] so they compile to nothing on other OSes.
// - Windows: serialport returns names like "COM3". No special logic; they list cleanly
//   with their VID/PID descriptions when USB.
// - Linux: common names /dev/ttyUSB*, /dev/ttyACM*, /dev/ttyS*. Refresh + selection
//   "just work". The existing code is already portable here.
// VID/PID descriptions are provided on all platforms via the UsbPort info when present.
// No behavior change for macOS; the code no longer "looks macOS-only" to readers.

use eframe::egui;
use serialport::SerialPortType;

#[cfg(target_os = "macos")]
fn is_macos_tty_port(name: &str) -> bool {
    name.contains("/tty.")
}

#[cfg(target_os = "macos")]
fn find_cu_equivalent(ports: &[SerialPortInfo], tty_name: &str) -> Option<String> {
    let cu_name = tty_name.replace("/tty.", "/cu.");
    ports
        .iter()
        .find(|p| p.port_name == cu_name)
        .map(|p| p.port_name.clone())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum DataSourceType {
    Synthetic,
    CytonSerial,
    CytonWifi,
    GanglionNative,
    Playback, // Phase 7 — record → End Session → immediate Playback of the exact file
}

impl DataSourceType {
    /// Finished take (Playback / SD). Live boards Start a session.
    pub fn is_finished_take(self) -> bool {
        matches!(self, Self::Playback)
    }

    /// WiFi shield / Ganglion — behind Session Setup Advanced.
    pub fn is_advanced(self) -> bool {
        matches!(self, Self::CytonWifi | Self::GanglionNative)
    }

    pub fn go_label(self) -> &'static str {
        if self.is_finished_take() {
            "Play"
        } else {
            "Start Session"
        }
    }
}

#[derive(Clone)]
pub struct SerialPortInfo {
    pub port_name: String,
    pub description: String,
}

/// USB-serial that is likely an OpenBCI FTDI dongle (not Bluetooth / debug consoles).
pub fn looks_like_cyton_dongle(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("bluetooth") || n.contains("debug") || n.contains("incoming") {
        return false;
    }
    n.contains("usbserial")
        || n.contains("ttyusb")
        || n.contains("ttyacm")
        || n.contains("usbmodem")
}

/// Prefer a still-plugged last port, then cu.usbserial / ttyUSB, then usbmodem.
pub fn pick_cyton_port(ports: &[SerialPortInfo], preferred: Option<&str>) -> Option<String> {
    if let Some(want) = preferred {
        if ports.iter().any(|p| p.port_name == want) {
            return Some(want.to_string());
        }
    }
    let mut cands: Vec<&SerialPortInfo> = ports
        .iter()
        .filter(|p| looks_like_cyton_dongle(&p.port_name))
        .collect();
    cands.sort_by_key(|p| {
        let n = p.port_name.to_ascii_lowercase();
        let tty = n.contains("/tty.");
        let usbserial = n.contains("usbserial") || n.contains("ttyusb") || n.contains("ttyacm");
        (tty, !usbserial, p.port_name.clone())
    });
    cands.first().map(|p| p.port_name.clone())
}

pub struct ControlPanel {
    pub selected_source: DataSourceType,
    pub synthetic_channels: usize,
    pub cyton_channels: usize, // 8 or 16 (Daisy)
    pub serial_ports: Vec<SerialPortInfo>,
    pub selected_serial_port: Option<usize>,
    pub show: bool,

    // Phase 7 Playback support
    pub playback_file: Option<String>,
    /// BrainFlow Ganglion Native identifier (MAC or advertised name). Empty = do not connect.
    pub ganglion_device_id: String,
    pub cyton_wifi_ip: String,
    pub ble_devices: Vec<crate::board::ble_scan::GanglionDevice>,
    pub ble_scan_status: Option<String>,
    pub last_setup_error: Option<String>,
    /// Gates Cyton WiFi + Ganglion. Always-on: Cyton Serial, Synthetic, Playback.
    pub show_advanced: bool,
}

impl ControlPanel {
    pub fn new() -> Self {
        let mut panel = Self {
            selected_source: DataSourceType::Synthetic,
            synthetic_channels: 8,
            cyton_channels: 8,
            serial_ports: vec![],
            selected_serial_port: None,
            show: true,
            playback_file: None,
            ganglion_device_id: String::new(),
            cyton_wifi_ip: String::new(),
            ble_devices: Vec::new(),
            ble_scan_status: None,
            last_setup_error: None,
            show_advanced: false,
        };
        panel.refresh_serial_ports();
        panel
    }

    pub fn pick_present_cyton_port(&self, preferred: Option<&str>) -> Option<String> {
        pick_cyton_port(&self.serial_ports, preferred)
    }

    pub fn refresh_serial_ports(&mut self) {
        // Phase 8 suggestion: capture prior selection *by name* before clearing so Refresh preserves
        // the user's (or persisted/Reconnect) choice even if port list order or indices change.
        let prior_name = self
            .selected_serial_port
            .and_then(|i| self.serial_ports.get(i))
            .map(|p| p.port_name.clone());

        self.serial_ports.clear();

        if let Ok(ports) = serialport::available_ports() {
            for p in ports {
                let desc = match &p.port_type {
                    SerialPortType::UsbPort(info) => {
                        format!(
                            "{} (VID:{:04x} PID:{:04x})",
                            info.product.as_deref().unwrap_or("USB"),
                            info.vid,
                            info.pid
                        )
                    }
                    _ => "Serial".to_string(),
                };
                self.serial_ports.push(SerialPortInfo {
                    port_name: p.port_name,
                    description: desc,
                });
            }
        }

        // On macOS, strongly prefer cu.* ports over tty.* ports for USB serial devices.
        // This is the #1 cause of "BrainFlow can't open port" errors with Cyton dongles.
        #[cfg(target_os = "macos")]
        self.serial_ports.sort_by(|a, b| {
            let a_is_cu = a.port_name.contains("/cu.");
            let b_is_cu = b.port_name.contains("/cu.");
            match (a_is_cu, b_is_cu) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.port_name.cmp(&b.port_name),
            }
        });

        // Best-effort re-select by the preserved name (name is the source of truth for persistence + Reconnect)
        if let Some(name) = prior_name {
            if let Some(idx) = self.serial_ports.iter().position(|p| p.port_name == name) {
                self.selected_serial_port = Some(idx);
            } else if !self.serial_ports.is_empty() && self.selected_serial_port.is_none() {
                self.selected_serial_port = Some(0);
            }
        } else if !self.serial_ports.is_empty() && self.selected_serial_port.is_none() {
            self.selected_serial_port = Some(0);
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) -> Option<(DataSourceType, usize, Option<String>)> {
        let mut result = None;

        // Persist Advanced open when an advanced source is already selected (e.g. restore).
        if self.selected_source.is_advanced() {
            self.show_advanced = true;
        }

        ui.vertical_centered(|ui| {
            // Soft vertical middle bias (Ableton quiet) — shrinks before crop.
            let soft_top = (ui.available_height() * 0.18).clamp(16.0, 72.0);
            ui.add_space(soft_top);

            const GLASS_OUTER: f32 = 480.0;

            let glass = egui::Frame::new()
                .fill(crate::theme::PANEL_RAISED)
                .stroke(egui::Stroke::new(1.0, crate::theme::HAIRLINE))
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(20, 16));

            ui.allocate_ui_with_layout(
                egui::vec2(GLASS_OUTER, 0.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.set_min_width(GLASS_OUTER);
                    ui.set_max_width(GLASS_OUTER);
                    glass.show(ui, |ui| {
                                // Content ~360 + 20 side margins => outer ≈400.
                                ui.set_max_width(440.0);
                                ui.set_min_width(400.0);
                                // Place lock: title + radios + Advanced + details as ONE centered block.
                                ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Data Source")
                                    .strong()
                                    .color(crate::theme::TEXT),
                            );
                            ui.add_space(6.0);

                // Always-on bare radios (not toggles). Order: Cyton Serial / Synthetic / Playback.
                self.setup_radio(ui, DataSourceType::CytonSerial, "Cyton (Serial / USB Dongle)");
                self.setup_radio(ui, DataSourceType::Synthetic, "Synthetic (BrainFlow)");
                self.setup_radio(ui, DataSourceType::Playback, "Playback (recording / Cyton SD)");

                ui.add_space(8.0);
                // Bare Advanced — no frame, no parenthetical chrome.
                if ui
                    .add(egui::Checkbox::new(&mut self.show_advanced, "Advanced"))
                    .changed()
                    && !self.show_advanced
                    && self.selected_source.is_advanced()
                {
                    self.selected_source = DataSourceType::Synthetic;
                }
                if self.show_advanced {
                    self.setup_radio(ui, DataSourceType::CytonWifi, "Cyton (WiFi shield)");
                    self.setup_radio(ui, DataSourceType::GanglionNative, "Ganglion (Native BLE)");
                }

                // Serial + channels live under Cyton Serial inside the same glass stack.
                match self.selected_source {
                    DataSourceType::Synthetic => {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Channels:");
                            ui.add(egui::DragValue::new(&mut self.synthetic_channels).range(1..=16));
                        });
                    }
                    DataSourceType::CytonSerial => {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Serial Port:");
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Refresh")
                                            .color(crate::theme::ACCENT_LIME),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.refresh_serial_ports();
                            }
                        });

                        if self.serial_ports.is_empty() {
                            ui.label(
                                "No serial ports found. Plug in your Cyton dongle and click Refresh.",
                            );
                        } else {
                            #[cfg(target_os = "macos")]
                            ui.label(
                                egui::RichText::new("e.g. cu.usbserial-XXXX (prefer cu.* over tty.*)")
                                    .italics()
                                    .small(),
                            );
                            #[cfg(not(target_os = "macos"))]
                            ui.label(
                                egui::RichText::new("Select the serial port for your Cyton USB dongle.")
                                    .italics()
                                    .small(),
                            );

                            egui::ComboBox::from_label("")
                                .selected_text(
                                    self.selected_serial_port
                                        .and_then(|i| self.serial_ports.get(i))
                                        .map(|p| format!("{} — {}", p.port_name, p.description))
                                        .unwrap_or_else(|| "Select port...".to_string()),
                                )
                                .show_ui(ui, |ui| {
                                    for (idx, port) in self.serial_ports.iter().enumerate() {
                                        let text =
                                            format!("{} — {}", port.port_name, port.description);
                                        if ui
                                            .selectable_label(
                                                self.selected_serial_port == Some(idx),
                                                text,
                                            )
                                            .clicked()
                                        {
                                            self.selected_serial_port = Some(idx);
                                        }
                                    }
                                });

                            #[cfg(target_os = "macos")]
                            if let Some(idx) = self.selected_serial_port {
                                if let Some(port) = self.serial_ports.get(idx) {
                                    if is_macos_tty_port(&port.port_name) {
                                        ui.add_space(6.0);
                                        ui.colored_label(
                                            egui::Color32::from_rgb(220, 60, 60),
                                            "⚠️  Wrong port type on macOS! Using tty.* almost always fails.",
                                        );
                                        ui.label(
                                            "You should use the cu.* version of this port instead.",
                                        );

                                        if let Some(cu_port) =
                                            find_cu_equivalent(&self.serial_ports, &port.port_name)
                                        {
                                            if ui
                                                .button(format!(
                                                    "Use {} instead (recommended)",
                                                    cu_port
                                                ))
                                                .clicked()
                                            {
                                                if let Some(cu_idx) = self
                                                    .serial_ports
                                                    .iter()
                                                    .position(|p| p.port_name == cu_port)
                                                {
                                                    self.selected_serial_port = Some(cu_idx);
                                                }
                                            }
                                        } else {
                                            ui.label(
                                                egui::RichText::new(
                                                    "Unplug + replug the dongle, then Refresh.",
                                                )
                                                .italics(),
                                            );
                                        }
                                    }
                                }
                            }

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label("Channels:");
                                ui.radio_value(&mut self.cyton_channels, 8, "8 ch");
                                ui.radio_value(&mut self.cyton_channels, 16, "16 ch");
                            });
                        }
                    }
                    DataSourceType::CytonWifi => {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.label(
                            "Cyton over WiFi shield (BrainFlow CYTON_WIFI_BOARD, port 6677).",
                        );
                        ui.small(
                            "Not verified on hardware in this build unless a shield is on the bench.",
                        );
                        ui.horizontal(|ui| {
                            ui.label("IP address:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cyton_wifi_ip)
                                    .desired_width(180.0)
                                    .hint_text("192.168.4.1"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Channels:");
                            ui.radio_value(&mut self.cyton_channels, 8, "8 ch");
                            ui.radio_value(&mut self.cyton_channels, 16, "16 ch (Daisy)");
                        });
                    }
                    DataSourceType::GanglionNative => {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.label("Ganglion (4 ch) via BrainFlow native BLE.");
                        ui.label(
                            egui::RichText::new(
                                "Enter the board MAC address or advertised name. Empty field does not connect.",
                            )
                            .italics()
                            .small(),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Device ID:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.ganglion_device_id)
                                    .desired_width(260.0)
                                    .hint_text("AA:BB:CC:DD:EE:FF or Ganglion-XXXX"),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Scan BLE").clicked() {
                                match crate::board::ble_scan::scan_ganglions(
                                    std::time::Duration::from_secs(3),
                                ) {
                                    Ok(list) => {
                                        if list.is_empty() {
                                            self.ble_scan_status = Some("none found".into());
                                        } else {
                                            self.ble_scan_status =
                                                Some(format!("{} device(s)", list.len()));
                                        }
                                        self.ble_devices = list;
                                    }
                                    Err(e) => {
                                        self.ble_scan_status = Some(e);
                                        self.ble_devices.clear();
                                    }
                                }
                            }
                            if let Some(ref s) = self.ble_scan_status {
                                ui.small(s);
                            }
                        });
                        if !self.ble_devices.is_empty() {
                            for d in &self.ble_devices {
                                let selected = self.ganglion_device_id == d.id
                                    || self.ganglion_device_id == d.name;
                                if ui
                                    .selectable_label(selected, format!("{}  {}", d.name, d.id))
                                    .clicked()
                                {
                                    self.ganglion_device_id = d.id.clone();
                                }
                            }
                        }
                    }
                    DataSourceType::Playback => {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("📁 Choose Recording File...").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Select recording or Cyton SD hex")
                                    .add_filter(
                                        "Recordings",
                                        &["parquet", "txt", "odf", "csv", "bdf", "log", "hex"],
                                    )
                                    .set_directory(
                                        std::env::current_dir()
                                            .unwrap_or_else(|_| std::path::PathBuf::from(".")),
                                    )
                                    .pick_file()
                                {
                                    self.playback_file = Some(path.display().to_string());
                                }
                            }
                            #[allow(clippy::collapsible_if)]
                            if self.playback_file.is_some() {
                                if ui.button("Clear").clicked() {
                                    self.playback_file = None;
                                }
                            }
                        });
                        if let Some(ref f) = self.playback_file {
                            let short = if f.len() > 60 {
                                format!("...{}", &f[f.len() - 57..])
                            } else {
                                f.clone()
                            };
                            ui.label(egui::RichText::new(format!("Selected: {}", short)).small());
                        } else {
                            ui.label(
                                egui::RichText::new(
                                    "Select a recording (.parquet / .bdf / .txt) or Cyton SD hex dump.",
                                )
                                .italics()
                                .small(),
                            );
                        }
                    }
                }
                                }); // end vertical_centered (title + control stack)
                            }); // end glass.show
                },
            ); // end centered glass

            ui.add_space(24.0);

            if let Some(ref err) = self.last_setup_error {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
                ui.add_space(8.0);
            }

            // Quiet ReBot Start: PANEL_RAISED fill + ACCENT_LIME label.
            let start = ui.add(
                egui::Button::new(
                    egui::RichText::new(self.selected_source.go_label())
                        .color(crate::theme::ACCENT_LIME)
                        .strong(),
                )
                .fill(crate::theme::PANEL_RAISED)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::ACCENT_LIME))
                .corner_radius(8.0)
                .min_size(egui::vec2(200.0, 36.0)),
            );
            if start.clicked() {
                self.last_setup_error = None;

                if self.selected_source == DataSourceType::GanglionNative
                    && self.ganglion_device_id.trim().is_empty()
                {
                    self.last_setup_error = Some(
                        "Ganglion: enter a MAC / device name. Synthetic is a separate source."
                            .into(),
                    );
                } else if self.selected_source == DataSourceType::Playback
                    && self.playback_file.as_ref().map(|s| s.is_empty()).unwrap_or(true)
                {
                    self.last_setup_error = Some("Choose a playback file first.".into());
                } else if self.selected_source == DataSourceType::CytonWifi
                    && self.cyton_wifi_ip.trim().is_empty()
                {
                    self.last_setup_error = Some("Enter the WiFi shield IP address.".into());
                } else {
                    let mut port = self
                        .selected_serial_port
                        .and_then(|i| self.serial_ports.get(i))
                        .map(|p| p.port_name.clone());

                    #[cfg(target_os = "macos")]
                    if let Some(ref p) = port {
                        if is_macos_tty_port(p) {
                            if let Some(cu) = find_cu_equivalent(&self.serial_ports, p) {
                                tracing::warn!(
                                    "Auto-corrected serial port {} → {} (macOS best practice)",
                                    p,
                                    cu
                                );
                                port = Some(cu);
                            }
                        }
                    }

                    if self.selected_source == DataSourceType::Playback {
                        port = self.playback_file.clone();
                    }
                    if self.selected_source == DataSourceType::GanglionNative {
                        port = Some(self.ganglion_device_id.trim().to_string());
                    }
                    if self.selected_source == DataSourceType::CytonWifi {
                        port = Some(self.cyton_wifi_ip.trim().to_string());
                    }

                    let channels = match self.selected_source {
                        DataSourceType::Synthetic => self.synthetic_channels,
                        DataSourceType::CytonSerial | DataSourceType::CytonWifi => {
                            self.cyton_channels
                        }
                        DataSourceType::GanglionNative => 4,
                        DataSourceType::Playback => 8,
                    };

                    result = Some((self.selected_source, channels, port));
                    self.show = false;
                }
            }
        });

        result
    }

    /// Radio with lime selection tick (ReBot). Not a toggle.
    fn setup_radio(&mut self, ui: &mut egui::Ui, value: DataSourceType, text: &str) {
        let checked = self.selected_source == value;
        let response = ui.add(egui::RadioButton::new(checked, text));
        if checked {
            let rect = response.rect.expand(3.0);
            ui.painter().rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(1.5_f32, crate::theme::ACCENT_LIME),
                egui::StrokeKind::Outside,
            );
        }
        if response.clicked() {
            self.selected_source = value;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::DataSourceType;

    #[test]
    fn pick_cyton_port_prefers_cu_usbserial() {
        use super::{pick_cyton_port, SerialPortInfo};
        let ports = vec![
            SerialPortInfo {
                port_name: "/dev/cu.Bluetooth-Incoming-Port".into(),
                description: "Bluetooth".into(),
            },
            SerialPortInfo {
                port_name: "/dev/tty.usbserial-TEST1".into(),
                description: "USB".into(),
            },
            SerialPortInfo {
                port_name: "/dev/cu.usbserial-TEST1".into(),
                description: "USB".into(),
            },
        ];
        assert_eq!(
            pick_cyton_port(&ports, None).as_deref(),
            Some("/dev/cu.usbserial-TEST1")
        );
        assert_eq!(
            pick_cyton_port(&ports, Some("/dev/cu.usbserial-TEST1")).as_deref(),
            Some("/dev/cu.usbserial-TEST1")
        );
        assert!(pick_cyton_port(&[], None).is_none());
    }

    #[test]
    fn play_is_finished_take_only() {
        assert_eq!(DataSourceType::Playback.go_label(), "Play");
        assert!(DataSourceType::Playback.is_finished_take());
    }

    #[test]
    fn live_boards_start_a_session() {
        for src in [
            DataSourceType::Synthetic,
            DataSourceType::CytonSerial,
            DataSourceType::CytonWifi,
            DataSourceType::GanglionNative,
        ] {
            assert_eq!(src.go_label(), "Start Session", "{src:?}");
            assert!(!src.is_finished_take(), "{src:?}");
        }
    }

    #[test]
    fn advanced_gates_wifi_and_ganglion_only() {
        assert!(DataSourceType::CytonWifi.is_advanced());
        assert!(DataSourceType::GanglionNative.is_advanced());
        assert!(!DataSourceType::Synthetic.is_advanced());
        assert!(!DataSourceType::CytonSerial.is_advanced());
        assert!(!DataSourceType::Playback.is_advanced());
    }

    #[test]
    fn session_setup_quiet_lime_start() {
        let src = include_str!("control_panel.rs");
        let impl_src = src.split("#[cfg(test)]").next().expect("impl before tests");
        let draw_body = impl_src
            .split("pub fn draw(")
            .nth(1)
            .expect("draw body");
        assert!(
            !draw_body.contains("Same boards"),
            "tagline must be gone from Session Setup"
        );
        assert!(
            !impl_src.contains("ensure_hero_icon")
                && !impl_src.contains("mind_harness_setup_hero")
                && !draw_body.contains("Image::new")
                && !draw_body.contains("300.0")
                && !draw_body.contains("PAIR")
                && draw_body.contains("GLASS_OUTER"),
            "Session Setup must be the quiet card only — no hero splash"
        );
        assert!(
            !draw_body.contains("add_space(72.0)"),
            "empty 72px hero slot must stay gone"
        );
        assert!(
            draw_body.contains("PANEL_RAISED"),
            "Data Source + Serial stack must sit on quiet charcoal card"
        );
        assert!(
            !draw_body.contains("SETUP_CYAN"),
            "Session Setup chrome must not use Imagine SETUP_CYAN"
        );
        assert!(
            draw_body.contains("ACCENT_LIME"),
            "selected radio / Start must use ACCENT_LIME"
        );
        assert!(
            draw_body.contains(".fill(crate::theme::PANEL_RAISED)")
                && draw_body.contains(".color(crate::theme::ACCENT_LIME)")
                && !draw_body.contains("SETUP_NEON")
                && !draw_body.contains("0xf2, 0xf7, 0xf8"),
            "Start Session must use PANEL_RAISED fill + ACCENT_LIME label, not SETUP_NEON"
        );
        assert!(
            draw_body.contains("Advanced"),
            "bare Advanced checkbox must gate WiFi + Ganglion"
        );
        assert!(
            draw_body.contains("show_advanced"),
            "Advanced flag must gate WiFi + Ganglion radios"
        );
        assert!(
            draw_body.contains("setup_radio"),
            "source pickers must be radios, not toggles"
        );
        assert!(
            draw_body.contains("Checkbox::new") || draw_body.contains("checkbox("),
            "Advanced must stay a bare checkbox"
        );
        // Always-on order: Cyton Serial before Synthetic before Playback in draw.
        let cyton = draw_body
            .find("DataSourceType::CytonSerial")
            .expect("Cyton Serial radio");
        let synth = draw_body
            .find("DataSourceType::Synthetic")
            .expect("Synthetic radio");
        let play = draw_body
            .find("DataSourceType::Playback")
            .expect("Playback radio");
        assert!(
            cyton < synth && synth < play,
            "always-on radios must be Cyton Serial / Synthetic / Playback"
        );
        // Place lock: whole control stack (title + radios + Advanced + match) in one
        // vertical_centered inside glass.show.
        let glass_at = draw_body.find("glass.show").expect("glass.show");
        let after_glass = &draw_body[glass_at..];
        let stack_vc = after_glass
            .find("ui.vertical_centered(|ui| {")
            .expect("vertical_centered inside glass");
        let in_stack = &after_glass[stack_vc..];
        assert!(
            in_stack.find("Data Source").expect("title")
                < in_stack.find("setup_radio").expect("radios"),
            "Data Source title and radios must share one vertical_centered inside glass"
        );
    }
}
