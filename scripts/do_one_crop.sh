#!/usr/bin/env bash
# One recapture: PLAYBACK seek (OPENBCI_PLAYBACK_SEEK_SEC, default 147.9) then OPENBCI_CROP ppm then sips png then kill this GUI.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NAME="${1:?name}"
LAYOUT="${2:?layout}"
GRID="${3:?grid}"
PPM="$ROOT/Recordings/${NAME}.ppm"
PNG="$ROOT/Recordings/${NAME}.png"
PLAY="$ROOT/Recordings/OpenBCI_2026-08-30_17-55-39_625_626066000_0.bdf"
rm -f "$PPM"
export OPENBCI_PLAYBACK="$PLAY"
export OPENBCI_PLAYBACK_SEEK_SEC="${OPENBCI_PLAYBACK_SEEK_SEC:-147.9}"
export OPENBCI_LAYOUT="$LAYOUT"
export OPENBCI_GRID="$GRID"
export OPENBCI_CROP="$PPM"
cd "$ROOT"
bash "$ROOT/scripts/run_crop.sh" &
PID=$!
echo "launched pid=$PID name=$NAME layout=$LAYOUT grid=$GRID"
ok=0
for i in $(seq 1 50); do
  if [[ -f "$PPM" ]]; then
    sz=$(stat -f%z "$PPM" 2>/dev/null || echo 0)
    if [[ "$sz" -gt 1000000 ]]; then
      sleep 3
      ok=1
      break
    fi
  fi
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "GUI died early pid=$PID" >&2
    break
  fi
  sleep 0.4
done
if [[ "$ok" -ne 1 ]]; then
  echo "NO PPM for $NAME (pid=$PID)" >&2
  ls -la "$PPM" 2>/dev/null || true
  kill "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
  exit 1
fi
sips -s format png "$PPM" --out "$PNG" >/dev/null
kill "$PID" 2>/dev/null || true
wait "$PID" 2>/dev/null || true
sleep 0.3
if kill -0 "$PID" 2>/dev/null; then
  kill -9 "$PID" 2>/dev/null || true
fi
echo "wrote $PNG $(stat -f%z "$PNG") bytes"
