#!/usr/bin/env bash
# Reviewable wrapper: Blender CLI on this Mac.
set -euo pipefail
BLENDER="${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}"
exec "$BLENDER" "$@"
