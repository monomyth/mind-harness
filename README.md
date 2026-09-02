# Mind Harness

A native Mac app for looking at EEG from an Ultracortex Mark IV. Record a sitting, play it back, and watch traces, spectra, and the headset on a head.

![Head Plot — Ultracortex Mark IV, default eight sites](docs/head-plot.png)

The Head Plot shows the Mark IV on a dummy head. The default eight inserts follow OpenBCI’s Cyton map: Fp1, Fp2, C3, C4, P7, P8, O1, O2. Empty inserts stay empty.

## Run

```bash
cd ~/code/grok/mind-harness
cargo run --release --bin mind-harness
```

BrainFlow’s native libraries live in `~/github/brainflow/rust_package/brainflow/lib` (or set `BRAINFLOW_LIB`). First compile is slow; later ones are not.

## What it talks to

- A synthetic board, for layout without hardware
- A Cyton over a USB serial dongle
- Playback of a recording from this app or the original OpenBCI GUI

Session holds layout, notch, and band. Record sits on the transport bar. Experiments is a guided-recording card, not a second home screen.

## License

MIT. See `LICENSE`. This is original work, inspired by the OpenBCI GUI (also MIT, 2018 OpenBCI). That notice is in `NOTICE`. Not affiliated with OpenBCI.
