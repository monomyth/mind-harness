#!/usr/bin/env bash
# Verify Mark IV disc centering + Head Plot color key, then recrop.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== cargo test =="
cargo test --offline -- --test-threads=1

echo "== cargo build release =="
cargo build --release --offline --bin mind-harness

echo "== recrop Head Plot =="
OPENBCI_PLAYBACK_SEEK_SEC=40 bash scripts/do_one_crop.sh plate_head_plot 1 "Head Plot"
cp -f Recordings/plate_head_plot.png docs/head-plot.png
ls -la Recordings/plate_head_plot.png docs/head-plot.png
echo "DONE center_holes_verify"
