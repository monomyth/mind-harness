#!/usr/bin/env bash
# Reviewable wrapper: launch the GUI with inherited recapture env (OPENBCI_*).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${BIN:-"$ROOT/target/release/mind-harness"}"
cd "$ROOT"
exec "$BIN" "$@"
