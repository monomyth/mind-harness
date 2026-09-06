#!/usr/bin/env bash
# Install Mind Harness + BrainFlow build/runtime deps on Arch Linux.
# Tested package set used on isengard for the 2.2.48 Linux build path.
# Other distros: map equivalents yourself; only Arch was verified.
#
# Usage (on the Linux host):
#   sudo bash scripts/install-mh-linux-deps.sh
#
# Then build BrainFlow with scripts/build_brainflow_linux.sh and
# cargo build --release --locked --bin mind-harness from a sibling layout.
set -euo pipefail
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  echo "Run: sudo bash $0"
  exit 1
fi
pacman -Sy --needed --noconfirm \
  cmake ninja \
  python \
  clang llvm \
  pkgconf \
  vulkan-icd-loader vulkan-headers \
  libxkbcommon libxkbcommon-x11 \
  libxcb xcb-util xcb-util-wm xcb-util-keysyms xcb-util-image xcb-util-renderutil \
  gtk3 \
  alsa-lib \
  libusb \
  bluez-libs \
  mesa \
  wayland wayland-protocols
echo "deps installed. Next: scripts/build_brainflow_linux.sh then cargo build --release --locked."
