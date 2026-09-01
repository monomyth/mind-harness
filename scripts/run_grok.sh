#!/usr/bin/env bash
# Reviewable wrapper so ExternalShell Auto-review can bind grok invokes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GROK_BIN="${GROK_BIN:-$HOME/.grok/downloads/grok-1.0.15-macos-aarch64}"
cd "$ROOT"
exec "$GROK_BIN" --cwd "$ROOT" "$@"
