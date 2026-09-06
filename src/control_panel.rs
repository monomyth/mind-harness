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
    SDCard,
}

impl DataSourceType {
    /// Finished take (Playback / SD). Live boards Start a session.
    pub fn is_finished_take(self) -> bool {
        matches!(self, Self::Playback | Self::SDCard)
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
    pub sd_file: Option<String>,
    pub ble_devices: Vec<crate::board::ble_scan::GanglionDevice>,
    pub ble_scan_status: Option<String>,
    pub last_setup_error: Option<String>,
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
            sd_file: None,
            ble_devices: Vec::new(),
            ble_scan_status: None,
            last_setup_error: None,
        };
        panel.refresh_serial_ports();
        panel
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

        ui.vertical_centered(|ui| {
            ui.heading(
                egui::RichText::new(format!(
                    "Mind Harness  v{}  —  Session Setup",
                    env!("CARGO_PKG_VERSION")
                ))
                .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new("Same boards, same experiment loop.")
                    .italics()
                    .color(crate::theme::TEXT),
            );
            ui.add_space(16.0);

            ui.group(|ui| {
                ui.label("Data Source");
                ui.radio_value(&mut self.selected_source, DataSourceType::Synthetic, "Synthetic (BrainFlow)");
                ui.radio_value(&mut self.selected_source, DataSourceType::CytonSerial, "Cyton (Serial / USB Dongle)");
                ui.radio_value(&mut self.selected_source, DataSourceType::CytonWifi, "Cyton (WiFi shield)");
                ui.radio_value(&mut self.selected_source, DataSourceType::GanglionNative, "Ganglion (Native BLE)");
                ui.radio_value(&mut self.selected_source, DataSourceType::Playback, "Playback (.parquet / .bdf / .txt)");
                ui.radio_value(&mut self.selected_source, DataSourceType::SDCard, "SD Card (Cyton hex dump)");
            });

            ui.add_space(10.0);

            match self.selected_source {
                DataSourceType::Synthetic => {
                    ui.horizontal(|ui| {
                        ui.label("Channels:");
                        ui.add(egui::DragValue::new(&mut self.synthetic_channels).range(1..=16));
                    });
                }
                DataSourceType::CytonSerial => {
                    ui.horizontal(|ui| {
                        ui.label("Serial Port:");
                        if ui.button("Refresh").clicked() {
                            self.refresh_serial_ports();
                        }
                    });

                    if self.serial_ports.is_empty() {
                        ui.label("No serial ports found. Plug in your Cyton dongle and click Refresh.");
                    } else {
                        // Phase 8: platform-aware guidance (the macOS warning only appears on macOS)
                        #[cfg(target_os = "macos")]
                        ui.label(egui::RichText::new("Always prefer ports starting with 'cu.usbserial' (tty. versions usually fail)").italics());
                        #[cfg(not(target_os = "macos"))]
                        ui.label(egui::RichText::new("Select the serial port for your Cyton USB dongle.").italics());

                        egui::ComboBox::from_label("")
                            .selected_text(
                                self.selected_serial_port
                                    .and_then(|i| self.serial_ports.get(i))
                                    .map(|p| format!("{} — {}", p.port_name, p.description))
                                    .unwrap_or_else(|| "Select port...".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for (idx, port) in self.serial_ports.iter().enumerate() {
                                    let text = format!("{} — {}", port.port_name, port.description);
                                    if ui.selectable_label(self.selected_serial_port == Some(idx), text).clicked() {
                                        self.selected_serial_port = Some(idx);
                                    }
                                }
                            });

                        // === macOS-specific warning + auto-fix (very important for Cyton) ===
                        #[cfg(target_os = "macos")]
                        if let Some(idx) = self.selected_serial_port {
                            if let Some(port) = self.serial_ports.get(idx) {
                                if is_macos_tty_port(&port.port_name) {
                                    ui.add_space(6.0);
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 60, 60),
                                        "⚠️  Wrong port type on macOS! Using tty.* almost always fails.",
                                    );
                                    ui.label("You should use the cu.* version of this port instead.");

                                    if let Some(cu_port) = find_cu_equivalent(&self.serial_ports, &port.port_name) {
                                        if ui.button(format!("Use {} instead (recommended)", cu_port)).clicked() {
                                            if let Some(cu_idx) = self.serial_ports.iter().position(|p| p.port_name == cu_port) {
                                                self.selected_serial_port = Some(cu_idx);
                                            }
                                        }
                                    } else {
                                        ui.label(egui::RichText::new("Unplug + replug the dongle, then Refresh.").italics());
                                    }
                                }
                            }
                        }

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label("Channels:");
                            ui.radio_value(&mut self.cyton_channels, 8, "8 ch (Cyton)");
                            ui.radio_value(&mut self.cyton_channels, 16, "16 ch (Cyton + Daisy)");
                        });
                    }
                }
                DataSourceType::CytonWifi => {
                    ui.label("Cyton over WiFi shield (BrainFlow CYTON_WIFI_BOARD, port 6677).");
                    ui.small("Not verified on hardware in this build unless a shield is on the bench.");
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
                            match crate::board::ble_scan::scan_ganglions(std::time::Duration::from_secs(3))
                            {
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
                    // Phase 7 Playback roundtrip UI (plan.md Phase 7 step 3)
                    // rfd native file picker for .txt / .odf recordings produced by this GUI or Java GUI.
                    ui.horizontal(|ui| {
                        if ui.button("📁 Choose Recording File...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Select OpenBCI recording (.parquet / .bdf / .txt)")
                                .add_filter(
                                    "OpenBCI Recordings",
                                    &["parquet", "txt", "odf", "csv", "bdf"],
                                )
                                .set_directory(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                                .pick_file()
                            {
                                self.playback_file = Some(path.display().to_string());
                            }
                        }
                        // Allow: the nested if is required for immediate-mode UI (button must only be drawn/clicked when a file is selected)
                        #[allow(clippy::collapsible_if)]
                        if self.playback_file.is_some() {
                            if ui.button("Clear").clicked() {
                                self.playback_file = None;
                            }
                        }
                    });
                    if let Some(ref f) = self.playback_file {
                        let short = if f.len() > 60 { format!("...{}", &f[f.len()-57..]) } else { f.clone() };
                        ui.label(egui::RichText::new(format!("Selected: {}", short)).small());
                    } else {
                        ui.label(egui::RichText::new("Select a .parquet / .bdf / .txt recording from this GUI or Java.").italics().small());
                    }
                }
                DataSourceType::SDCard => {
                    ui.label("Cyton SD hex dump (Java DataSourceSDCard: comma-separated 24-bit hex).");
                    ui.horizontal(|ui| {
                        if ui.button("Choose SD file...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Select Cyton SD file")
                                .add_filter("SD / text", &["txt", "csv", "log", "hex"])
                                .pick_file()
                            {
                                self.sd_file = Some(path.display().to_string());
                            }
                        }
                        if self.sd_file.is_some() && ui.button("Clear").clicked() {
                            self.sd_file = None;
                        }
                    });
                    if let Some(ref f) = self.sd_file {
                        ui.small(format!("Selected: {f}"));
                    } else {
                        ui.small("Pick a Cyton SD hex file. Wrong format returns a readable error.");
                    }
                }
            }

            ui.add_space(24.0);

            if let Some(ref err) = self.last_setup_error {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
            }

            let start = ui.add(
                egui::Button::new(self.selected_source.go_label())
                    .fill(crate::theme::TURN_ON_GREEN)
                    .min_size(egui::vec2(180.0, 28.0)),
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
                } else if self.selected_source == DataSourceType::SDCard
                    && self.sd_file.as_ref().map(|s| s.is_empty()).unwrap_or(true)
                {
                    self.last_setup_error = Some("Choose a Cyton SD hex file first.".into());
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
                    if self.selected_source == DataSourceType::SDCard {
                        port = self.sd_file.clone();
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
                        DataSourceType::SDCard => 8,
                    };

                    result = Some((self.selected_source, channels, port));
                    self.show = false;
                }
            }
        });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::DataSourceType;

    #[test]
    fn play_is_finished_take_only() {
        assert_eq!(DataSourceType::Playback.go_label(), "Play");
        assert_eq!(DataSourceType::SDCard.go_label(), "Play");
        assert!(DataSourceType::Playback.is_finished_take());
        assert!(DataSourceType::SDCard.is_finished_take());
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
}
