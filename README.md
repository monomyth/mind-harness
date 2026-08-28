# OpenBCI GUI — Rust Native (macOS)

This is the in-progress native macOS rewrite of the OpenBCI GUI, previously implemented in Processing/Java.

**Status**: Native rewrite in progress. See `PORT_STATUS.md`. Core experiment loop (Synthetic / Cyton serial / Playback, Time Series, FFT, recording, filters) works. eframe 0.32 is required on macOS 26.

## Why Rust?

- Mature official BrainFlow Rust bindings (the critical dependency for Cyton/Ganglion hardware)
- Excellent `lsl` crate for LabStreamingLayer
- `egui` + `wgpu` (Metal) gives high-performance real-time plotting on Apple Silicon
- No need to maintain Windows/Linux builds for this effort

The original Java/Processing code in `~/github/OpenBCI_GUI/OpenBCI_GUI/` remains as the reference implementation and is **not modified** during this transition.

## Project Structure

```
openbci-gui-rust/
├── Cargo.toml
├── src/
│   ├── main.rs          # eframe entry point
│   ├── app.rs           # Main App state + system modes (future)
│   ├── board/           # DataSource trait + concrete boards (Synthetic, CytonSerial, GanglionNative...)
│   ├── widgets/         # TimeSeries, FFT, Networking, Focus, etc.
│   ├── widget_manager.rs
│   ├── data_writers/    # BDF, ODF, BF writers
│   └── networking/      # OSC, UDP, LSL, Serial output
├── resources/           # Fonts + icons (copied from Java data/)
└── assets/              # macOS .icns icon
```

## Building & Running (Development)

```bash
cd ~/code/grok/openbci-gui-rust
cargo run --release
```

BrainFlow native libraries (`libBoardController.dylib` and friends) live in
`~/github/brainflow/rust_package/brainflow/lib`. The crate `build.rs` adds that
directory as an rpath. Override with `BRAINFLOW_LIB=/path/to/lib` if needed.

First run will download and compile ~400 crates (wgpu, metal, etc.). Subsequent builds are fast.

## Building a macOS .app Bundle (Phase 8 foundations)

The project already ships with bundler metadata in `Cargo.toml`:

```toml
[package.metadata.bundle]
name = "OpenBCI GUI"
identifier = "com.openbci.gui"
icon = ["resources/icon.icns"]  # place sketch.icns (from Java OpenBCI_GUI/sketch.icns) here; `cargo bundle` requires a real .icns for foundations
...
```

- Install: `cargo install cargo-bundle`
- Build: `cargo bundle --release` (from openbci-gui-rust/)
- Icon: copy `sketch.icns` (or a 1024px square .icns) into `resources/` (fonts live here too) and point metadata at it.
- Result: `target/release/bundle/osx/OpenBCI GUI.app`

For DMG: `create-dmg` or `hdiutil` scripts can be added later in `scripts/`. Windows/Linux use `cargo build --release` (portable). No code changes needed for the foundations.

## Phase Plan (Summary) — Updated 2026-05-16

- **Phase 0–3** — Complete (Synthetic + real Cyton, widget system, layouts)
- **Phase 4** — Networking (UDP/OSC configurable + WidgetContext) — Complete
- **Phase 6** — Focus (real BrainFlow MLModel + graceful proxy + lock-free audio) — Complete
- **Phase 7** — **EventLog + rich global Console** (filter/search/save/copy), embedded Montserrat/OpenSans fonts, full Playback roundtrip (rfd picker + parser for Rust/Java ODF .txt, interactive pause/speed/seek controls in status bar), hybrid SidePanel layout (Focus/Networking/Marker/PacketLoss sparkline *always visible*), one-click Reconnect after failures, SD stub + honest docs. The "record a session → End → pick the exact file → replay with live Focus audio + markers + Networking + full Console audit" loop is now rock-solid. **Production complete.**
- **Phase 8** — Persistence (directories + JSON config for source/channels/port/file/filters/networking/recording-format; silent load, save on start/end), cross-platform serial port UX + comments, visible `v{CARGO_PKG_VERSION}` in UI + window title, release packaging foundations (Cargo.toml [package.metadata.bundle] already present for `cargo bundle`, icon in resources/ or assets/, minimal docs). **Release candidate — ready for daily use and distribution.**

See `PORT_STATUS.md` for the detailed feature parity table and the magical Playback roundtrip test procedure.

## Reference

The canonical behavior and file formats (BDF header layout, ODF columns, LSL stream names, etc.) are defined in the original Java implementation:

- `~/github/OpenBCI_GUI/OpenBCI_GUI/DataSource.pde`
- `~/github/OpenBCI_GUI/OpenBCI_GUI/Board*.pde`
- `~/github/OpenBCI_GUI/OpenBCI_GUI/W_TimeSeries.pde`
- `~/github/OpenBCI_GUI/OpenBCI_GUI/DataWriterBDF.pde`
- `~/github/OpenBCI_GUI/OpenBCI_GUI/W_Networking.pde`

All new Rust code aims for byte-for-byte compatible output files and identical user-facing behavior.

## License

MIT (same as the original OpenBCI GUI project)
