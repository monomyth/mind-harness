#!/usr/bin/env bash
# Reviewable wrapper: headless Cyton ingest probe (no GUI).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${BIN:-"$ROOT/target/release/cyton-probe"}"
PORT="${1:-${OPENBCI_LIVE_SERIAL:-}}"
SECS="${2:-12}"
if [[ -z "$PORT" ]]; then
    echo "usage: $0 <port> [seconds]" >&2
    echo "   or: OPENBCI_LIVE_SERIAL=<port> $0 [seconds]" >&2
    exit 2
fi
cd "$ROOT"
exec "$BIN" "$PORT" "$SECS"
