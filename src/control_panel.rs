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
    GanglionNative,
    Playback, // Phase 7 — record → End Session → immediate Playback of the exact file
    SDCard,   // Phase 7 stub — honest "not implemented yet"
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
                    "OpenBCI GUI  v{}  —  Session Setup",
                    env!("CARGO_PKG_VERSION")
                ))
                .color(crate::theme::OPENBCI_BLUE),
            );
            ui.label(
                egui::RichText::new("Native rewrite of the Processing GUI. Same boards, same experiment loop.")
                    .italics()
                    .color(crate::theme::OPENBCI_DARKBLUE),
            );
            ui.add_space(16.0);

            ui.group(|ui| {
                ui.label("Data Source");
                ui.radio_value(&mut self.selected_source, DataSourceType::Synthetic, "Synthetic (BrainFlow)");
                ui.radio_value(&mut self.selected_source, DataSourceType::CytonSerial, "Cyton (Serial / USB Dongle)");
                ui.radio_value(&mut self.selected_source, DataSourceType::GanglionNative, "Ganglion (Native BLE)");
                ui.radio_value(&mut self.selected_source, DataSourceType::Playback, "Playback (recorded .txt / .odf)");
                ui.radio_value(&mut self.selected_source, DataSourceType::SDCard, "SD Card (from board SD)");
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
                DataSourceType::GanglionNative => {
                    ui.label("Ganglion (4 ch) via BrainFlow native BLE.");
                    ui.label(
                        egui::RichText::new(
                            "Enter the board MAC address or advertised name. This port does not silently fall back to Synthetic.",
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
                }
                DataSourceType::Playback => {
                    // Phase 7 Playback roundtrip UI (plan.md Phase 7 step 3)
                    // rfd native file picker for .txt / .odf recordings produced by this GUI or Java GUI.
                    ui.horizontal(|ui| {
                        if ui.button("📁 Choose Recording File...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Select OpenBCI recording (.txt / .odf)")
                                .add_filter("OpenBCI Recordings", &["txt", "odf", "csv"])
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
                        ui.label(egui::RichText::new("Select a .txt recording made with 'Record (ODF)' or Java GUI equivalent.").italics().small());
                    }
                }
                DataSourceType::SDCard => {
                    // Phase 7 honest stub (plan.md Phase 7 step 7)
                    ui.colored_label(egui::Color32::from_rgb(200, 120, 60), "⚠️ SD Card reader not yet implemented in the Rust port.");
                    ui.label(egui::RichText::new("Use Record (BDF/ODF) + Playback instead — it gives you the exact same data with full widget + Console + Networking support and is more reliable for experiments.").small().italics());
                    ui.label(egui::RichText::new("See PORT_STATUS.md for details and the recommended workflow.").small());
                }
            }

            ui.add_space(24.0);

            if let Some(ref err) = self.last_setup_error {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
            }

            let start = ui.add(
                egui::Button::new("Start Session")
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
                } else if self.selected_source == DataSourceType::SDCard {
                    self.last_setup_error = Some(
                        "SD Card reader is not implemented. Record BDF/ODF and use Playback."
                            .into(),
                    );
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

                    let channels = match self.selected_source {
                        DataSourceType::Synthetic => self.synthetic_channels,
                        DataSourceType::CytonSerial => self.cyton_channels,
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
