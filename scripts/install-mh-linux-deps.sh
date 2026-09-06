#!/usr/bin/env bash
# Run on isengard:  sudo bash /home/monomyth/install-mh-linux-deps.sh
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
echo "deps installed. Rust Performance can continue the BrainFlow + cargo build."
