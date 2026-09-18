# Grok Build plan — top 10 gaps vs Java OpenBCI GUI

CWD: this repository
Reference Java: OpenBCI GUI (https://github.com/OpenBCI/OpenBCI_GUI)
Do not edit the Java tree.

This is **not** a 1:1 port of every widget. Order is daily-driver Cyton + honest hardware + decoder-ready recordings. Layouts 7–12 and Java chrome clones are out of scope unless a later item names them.

PORT_STATUS.md is mostly honest. Do not mark an item done because a widget exists. Done means the test at the bottom of that item passes on this machine.

## Rules

- One item per `/implement` run. Commit after each green item.
- Never claim live impedance, LSL, or WiFi unless a real Cyton/Ganglion session showed it.
- Markers must be sample-index accurate in the file, not just a unix timestamp in the widget.
- Do not train or ship an LLM on raw EEG. If you add an export, it is features + labels only.
- Keep the existing Synthetic / Cyton serial / ODF playback / record loop working. `cargo test` and `cargo clippy --all-targets -- -D warnings` (ignore pre-existing FFI noise) after each item.

---

## 1. Live ADS1299 impedance (kΩ)

**Now:** `startimp`/`stopimp` go out via `config_board`. Values are synthesized for Synthetic/Playback and labelled simulated. Live Cyton/Ganglion does not show real kΩ.

**Do:** Read Java `W_CytonImpedance.pde` + BrainFlow impedance docs. Parse real impedance from the board stream or the confirmed BrainFlow API. UI already exists (`src/widgets/impedance.rs`). If BrainFlow cannot return real kΩ, keep the simulated label and fail closed — do not paint fake green contacts on a live board.

**Done when:** On a live Cyton, Start Impedance shows per-channel kΩ that change when a lead is lifted, and the UI does **not** say simulated. Synthetic/Playback still say simulated.

## 2. Hardware Settings (channel on/off, gain, SRB, bias)

**Now:** Missing. Java has this next to impedance.

**Do:** Port the Cyton channel command set (x…X etc.) through `config_board`. Per-channel: power, gain, input type, SRB1/2, bias. Persist with the Phase 8 JSON. Do not invent Daisy commands you did not verify.

**Done when:** You can turn ch8 off and see it drop in Time Series on live Cyton (or Synthetic if you emulate the same command path in tests). Settings survive End Session → Start.

## 3. Sample-accurate markers in ODF + BDF

**Now:** `WMarker` sends unix time through `WidgetContext`. File comment still says BDF annotations are future. Decoder work needs sample index + timestamp aligned to the recording clock.

**Do:** On mark: write (sample_index, board_timestamp, label) into ODF as a column or sidecar and into BDF annotations. Playback must show those marks on the timeline. UDP/OSC already get markers — keep that.

**Done when:** Record 10s Synthetic, send 3 named marks, End, open the file: each mark is within 1 sample of the intended index. Playback draws them.

## 4. LSL output on macOS

**Now:** Checkbox disabled. `lsl_stream.rs` exists; networking widget says “LSL is not built in this binary.”

**Do:** Get `lsl` linking on this Mac (rpath / brew `/usr/local` or BrainFlow’s bundled liblsl). One outlet: EXG at the board rate, plus a marker stream. Same names/types as Java `W_Networking.pde` if they exist.

**Done when:** `cargo run` can enable LSL, LabRecorder (or `lsl_inlet` script) sees the stream, and 1s of Synthetic EXG arrives. If link fails, leave it disabled and document the exact linker error — do not show a green LSL checkbox.

## 5. BDF playback

**Now:** Playback is ODF/.txt. BDF is record-only.

**Do:** Parse the BDF this app writes (and Java BDF if cheap). Same PlaybackBoard controls (pause/seek/speed).

**Done when:** Record BDF on Synthetic → End → Playback that file → Time Series + marks match the session.

## 6. Cyton aux: Analog / Digital / Pulse

**Now:** Java `W_AnalogRead`, `W_DigitalRead`, `W_PulseSensor` are absent. BrainFlow already delivers those rows.

**Do:** Three small widgets that plot the aux channels Java uses for Cyton. Hide them on Synthetic if those channels do not exist. No fake pulse on Synthetic.

**Done when:** Live Cyton with something on D11/D12 or analog in shows a moving trace. Widget hidden or “no aux” on Synthetic.

**Parked 2026-08-29:** Eugene skipped live aux. Widgets exist; do not claim production until a pulse sensor or D11/D12/analog wire moves the trace. TODO: unpark when that hardware is on the bench.

## 7. Cyton WiFi (BrainFlow streaming board)

**Now:** Control panel is Synthetic / Cyton serial / Ganglion MAC / Playback / SD stub.

**Do:** Add Cyton WiFi: IP + board id, BrainFlow `CYTON` + WiFi shield params as Java does. Reuse the same session start path.

**Done when:** Control panel can start a WiFi Cyton without compiling a second binary. If no shield is on the bench, unit-test the config JSON and leave a “unverified on hardware” note in PORT_STATUS — do not claim production.

**Parked 2026-08-29:** No WiFi shield on the bench. TODO: unpark when one is here. Do not claim production off config JSON alone.

## 8. Ganglion BLE scanner

**Now:** Manual MAC/name field. No scan.

**Do:** BrainFlow discovery list → picker. Empty field still means do not connect (keep that fail-closed).

**Done when:** Scan lists a nearby Ganglion (or shows a clear “none found”). Selecting it fills the id and Start uses it.

**Parked 2026-08-29:** No Ganglion on the bench. TODO: unpark for a real scan (or an explicit none-found coverage pass). Empty BLE scan is optional, not the decoder path.

## 9. SD card reader

**Now:** Honest stub.

**Do:** Java SD playback path: pick a Cyton SD file, parse, play through PlaybackBoard. If format is ugly, do one well-tested Cyton SD layout, not five.

**Done when:** A known-good Cyton SD file plays in Time Series. Wrong file = a readable error, not a hang.

**Parked 2026-08-29:** No Cyton SD file on disk. TODO: unpark when a known-good card dump is here.

## 10. Labeled feature export (decoder prep, not an LLM)

**Now:** Recordings are raw ODF/BDF. Q2 needs epochs, not a language model on EEG.

**Do:** Optional export: windowed band power (or existing Focus features) + marker labels + artifact flag → CSV/JSONL next to the recording. No model training in this repo. No “intention” class unless the operator typed that marker.

**Done when:** After a marked Synthetic session, export a file where each row is `[t0, t1, ch, delta, theta, alpha, beta, gamma, marker, artifact]`. Document the schema in README. A later decoder can train on that; Grok does not.

---

## Suggested `/implement` sequence

1 → 2 → 3 → 5 → 4 → 10 → 6 → 8 → 7 → 9

Impedance + hardware settings + marks + BDF playback unlock a trustworthy archive. LSL and the export unlock other tools. Aux / BLE / WiFi / SD are hardware coverage.

After each item: update the matching row in `PORT_STATUS.md` with what was **verified**, not what was coded.
