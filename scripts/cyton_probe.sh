#!/usr/bin/env bash
# Reviewable wrapper: headless Cyton ingest probe (no GUI).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${BIN:-"$ROOT/target/release/cyton-probe"}"
PORT="${1:-/dev/cu.usbserial-DN00967F}"
SECS="${2:-12}"
cd "$ROOT"
exec "$BIN" "$PORT" "$SECS"
