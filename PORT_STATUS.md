# OpenBCI GUI Rust Port — Feature Parity & Status

**Date**: 2026-08-28 (v0.3.0 — GROK_PLAN items 1–10 coded; hardware claims only where verified)  
**Current State**: The May 2026 Phase 8 tree is the base. This pass fixed launch and data-path bugs that would have produced wrong recordings / silent hardware lies, then aligned chrome and default layout with the Java GUI. v0.3.0 adds the top-10 Cyton daily-driver gaps (impedance path, hardware settings, sample-accurate marks, BDF playback, LSL, feature export, aux widgets, BLE picker, WiFi config, SD hex).

**Launch notes (macOS 26)**: eframe 0.28 crashed in `NSScreen` enumeration (`q` vs `Q`). The GUI now uses **eframe/egui 0.32 + egui_plot 0.33** (winit 0.30.12+). `build.rs` embeds an rpath to BrainFlow's `lib/` so `target/debug/openbci_gui` loads `libBoardController.dylib`.

This document tracks parity with the canonical Java/Processing implementation (`~/github/OpenBCI_GUI/OpenBCI_GUI/`).

## Core Experiment Loop (Fully Working)

| Feature                        | Rust Status          | Notes |
|--------------------------------|----------------------|-------|
| Synthetic board                | ✅ Production        | BrainFlow Synthetic, 8/16 ch |
| Cyton Serial (cu.* preferred)  | ✅ Production        | Background connect, friendly errors |
| Time Series (individual ch)    | ✅ Production        | WTimeSeries |
| FFT                            | ✅ Production        | WFFT with rustfft |
| Band Power                     | ✅ Production        | WBandPower (authoritative exg_channels) |
| Accelerometer                  | ✅ Production        | WAccelerometer |
| Head Plot (topographic)        | ✅ **New**           | WHeadPlot — 2D head + electrodes, power-colored, in Tools panel |
| Marker (send + BDF + Net)      | ✅ Production        | Sample index + board time in ODF `% MARKER,idx,t,label`, sidecar `.markers.jsonl`, BDF+ TAL. Playback draws marks. UDP/OSC/LSL still get the mark. Verified on Synthetic ODF/BDF roundtrip (within 1 sample). |
| Networking (UDP/OSC/LSL)       | ⚠️ LSL linked here   | UDP/OSC as before. LSL uses Homebrew `lsl.framework` (`obci_eeg1` / EEG + `obci_markers` / Markers). Unit test creates an outlet and pulls 1 Synthetic-style sample. **LabRecorder on a live session: not run this pass.** If liblsl is missing at build, the checkbox stays disabled. |
| Focus (ML + proxy + audio)     | ✅ Production        | Phase 6 — real BrainFlow MLModel, lock-free cpal, threshold, Test Tone |
| **EventLog + Console**         | ✅ **Phase 7**       | Filterable, searchable, live, Save with rfd, 8 categories, mini-preview in status |
| **Playback roundtrip**         | ✅ **Phase 7**       | Parser for Rust ODF .txt (and Java), rfd picker, "record → End → pick exact file" flow, logs to Console, widgets receive data intent |
| Recording (BDF + ODF)          | ✅ Production        | DataLogger + BDF writer with TAL + ODF comments + sidecar |
| Impedance (Cyton ADS1299)      | ⚠️ Code complete     | Java lead-off: `x…Xz…Z` one channel at a time, kΩ = `(√2·std_µV·1e-6)/6nA − 2.2kΩ`. Ganglion: BrainFlow `z`/`Z` + `resistance_channels`/2. Synthetic/Playback still labelled simulated. **Not verified on a live Cyton this run** — do not treat as production until a lead-lift changes kΩ on hardware. |
| Filtering (Notch + BP)         | ✅ Production        | Notch: None / 50 / 60 / 50+60 (Java labels); BP 1–50 Hz; live + Playback |
| EMG                            | ✅ Production        | WEmg — envelope circles + 0–1 bar, Java EmgSettingsValues |
| EMG Joystick                   | ✅ Production        | WEmgJoystick — ±X/±Y channel map, unit-circle + lerp |

## Widget System & Layout

| Item                           | Status               | Notes |
|--------------------------------|----------------------|-------|
| Widget trait + Context         | ✅                   | — |
| Hybrid SidePanel + viz grid    | ✅ **Phase 7**       | 4 viz (TS/FFT/BP/Accel) in resizable Central grid + 3+ tools (Focus/Networking/Marker + PacketLoss sparkline) always visible in right panel. Console is global 📜 button + rich window (best UX). |
| WPacketLoss visual             | ✅ **Phase 7**       | Sparkline (60-sample history), color-coded %, Reset button (logs to EventLog). App-level heuristic + Playback = 0%. |
| All 12 Java layouts            | Partial (5 good ones) | Future (hybrid makes the 8-widget problem irrelevant) |

## Resilience & Polish (Phase 7+)

- **Reconnection**: ✅ **Phase 7** — Prominent red Failed banner + 🔄 Reconnect button that restores last source/port/channels/playback file. 1-click full reconnect for Synthetic & Playback (the magic roundtrip). For real Cyton it restores the exact dropdowns so the normal Start button succeeds on the second try. Survives End Session.
- **WPacketLoss**: ✅ Visual sparkline + Reset in SidePanel (Phase 7).
- **Fonts**: ✅ Embedded Montserrat + OpenSans (professional look matching Java GUI).
- **Hardware Settings**: Cyton/Synthetic ADS1299 `x…X` (power/gain/input/bias/SRB2). Persisted in Phase 8 JSON. Synthetic zeros powered-off EXG (ch8 off test). Live Cyton `config_board` **not hardware-verified this run**.
- **BDF playback**: `PlaybackBoard` reads the BDF this app writes (24-bit, 1 s records, TAL marks).
- **Feature export**: `Export features` writes `*.features.csv` + `*.features.jsonl` (`t0,t1,ch,delta,theta,alpha,beta,gamma,marker,artifact`). No model training.
- **Analog / Digital / Pulse**: Cyton-only; Synthetic shows “no aux”. `/2` analog `/3` digital. **Not verified on live D11/D12 this run.**
- **Ganglion BLE scan**: `system_profiler SPBluetoothDataType` picker; empty = “none found”; empty MAC still fail-closed.
- **Cyton WiFi**: Control panel IP + `CYTON_WIFI_BOARD` / Daisy WiFi, port 6677. Config unit-tested. **Unverified on hardware.**
- **SD Card**: Java hex layout (24-bit counts → µV). Wrong file = readable error. Verified with a synthetic hex fixture.
- **TopNav / Status icons**: Improved with Console button, REC, Net, Loss %, Last Marker, event count, Reconnect affordance.
- **Phase 8 + Post-Phase 8 wave polish**:
  - Persistence of last-used settings (Phase 8)
  - Graph speed & stability controls: **Time Window** (1s–60s) + **Y-Scale** locking (±50–1000 µV) in TimeSeries + **Smoothing** (0.0–0.999 exponential) for FFT & BandPower. These directly solve "graphs too fast/jerky" and are fully persisted.
  - Cross-platform serial docs, version strings, top bar layout hardening (no more frame-counter jerk, wrapped layout), release packaging metadata ready.
  - **Post-Phase 8 "Finish the Wave" (2026-05-17)**: WHeadPlot (iconic topographic map, always-visible in Tools side panel, works in Playback), Per-channel y-scale +/- controls on every TimeSeries channel row (classic ChannelBar experience + persisted), full Impedance UI widget (Start/Stop + color-coded kΩ readings per channel, real trait on DataSource + simulated for Playback + Cyton/Ganglion detection), top bar compaction (replaced 5 layout buttons with ComboBox, significantly shorter even on 1280px). All four items at the same production quality bar as Phase 8 (persistence, clean clippy, no regressions to existing widgets/Playback/filters/reconnect).

## How to Test the Magical Playback Roundtrip (Phase 7)

1. Start Synthetic (or Cyton).
2. Open **Console** (bottom bar) — watch it fill with "System", "Connection", "Filter" events.
3. Enable a couple of Networking toggles (UDP/OSC) — see them appear in Console.
4. Click **Record** (BDF or ODF).
5. Send a few Markers, toggle Focus audio or "Mark Current Focus Event".
6. Watch Console: "Recording started", "Marker: ...", "Focus: Manual...", "Networking enabled".
7. Click **End Session**.
8. Back in Control Panel, select **Playback (recorded .txt / .odf)**.
9. Click "Choose Recording File...", pick the exact file you just created in `Recordings/`.
10. Click **Start Session**.
11. Console immediately logs "Playback started from ...".
12. All widgets appear, Focus audio responds to the replayed EEG, you can still send Markers (they go to Networking + would annotate if we were re-recording), TimeSeries shows the recorded data, Networking continues to stream the replayed values.
13. The full closed-loop neurofeedback + audit-trail experiment is now reproducible without hardware.

## Persistence (Phase 8)

Last-used settings are persisted across app restarts for delightful QOL:
- Data source choice (Synthetic / Cyton / Playback / ...), channel counts (8/16), last serial port full path, last Playback file path.
- Recording format preference (BDF vs ODF).
- Global filter toggles (Notch 50/60, Bandpass 1-50).
- Networking: UDP and OSC enabled states + their target host:port strings.

**Storage**: JSON at the platform config dir returned by `directories::ProjectDirs` (e.g. on macOS `~/Library/Application Support/gui-rust/config.json`; on Linux `~/.config/openbci-gui-rust/config.json`; Windows `%APPDATA%\gui-rust\config.json`). Load failures are silent (debug trace only) and always fall back to safe defaults. Save happens on every successful "Start Session" and on "End Session".

**How to test persistence**:
1. Launch the app. Choose Cyton (or Synthetic 16ch), pick a specific port (or file), enable UDP with custom target, set Notch off, choose ODF recording, Start Session (or just change and End if already running).
2. Quit the app completely.
3. Re-launch: the Control Panel should show the exact same source, channels, port/file selection, and the SidePanel networking + filter checkboxes reflect last choices (networking targets restored in the widget too).
4. The `config.json` is human-readable; you can edit or delete it to reset.

## Remaining High-Value Items

- **Impedance on live Cyton/Ganglion**: Start/Stop no longer send the bogus `startimp`/`stopimp`. Cyton uses the Java ADS1299 lead-off command set and std→kΩ formula; Ganglion uses `z`/`Z` and resistance columns. Live boards fail closed (`None`, no simulated banner, no fake green contacts). Synthetic/Playback remain labelled simulated. **Hardware verification: not done this run** (no live Cyton session showed kΩ changing on a lifted lead).
- **Live Cyton Hardware Settings / analog pins / WiFi shield**: coded to Java command/config shapes; **not claimed production** until a board session shows it.
- **Layouts 7–12** (five- and six-pane Java maps) and drag-reorder are still future work. Layouts **1–6 match Java geometry**; default is Java layout 5 (tall left + two right).
- Ganglion BLED112 dongle path is still not a separate control-panel source (Native BLE + scanner only).

The Rust port is a usable daily driver for Synthetic, Cyton serial, Playback, recording, filters, and the core viz widgets. It is **not** a complete 1:1 of every Java widget.

See `README.md` for build/run/packaging instructions.
