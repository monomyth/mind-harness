#!/usr/bin/env bash
#
# Cloud Agent / Linux (Ubuntu) development environment setup for Mind Harness.
#
# Mind Harness is a native EEG instrument written in Rust (eframe/egui + wgpu)
# that talks to OpenBCI hardware through the BrainFlow C++ core. The app was
# authored on macOS, so a few things have to be recreated to build on Linux:
#
#   1. System libraries for egui/winit (X11/XCB, Wayland, Vulkan), rfd (GTK),
#      cpal (ALSA) and serialport (udev/libusb).
#   2. A recent Rust toolchain — some transitive dependencies require the
#      `edition2024` cargo feature (Rust >= 1.85).
#   3. The BrainFlow shared libraries. Cargo.toml points at a path dependency
#      under /Users/monomyth/github/brainflow, so we recreate that exact path,
#      clone BrainFlow, and build its C++ core (which drops the .so files into
#      rust_package/brainflow/lib where the Rust binding and build.rs expect
#      them).
#
# The script is idempotent: re-running it skips work that is already done.
set -euo pipefail

BRAINFLOW_HOME="/Users/monomyth/github/brainflow"
BRAINFLOW_RUST_LIB="$BRAINFLOW_HOME/rust_package/brainflow/lib"
BRAINFLOW_REF="5e81b6a2e461012ef94acb1772ad5249728baa1b" # pinned to a known-good BrainFlow commit
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> [1/4] Installing system packages (apt)"
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -qq
sudo apt-get install -y -qq \
  build-essential cmake ninja-build python3 clang llvm pkg-config \
  libstdc++-14-dev \
  libvulkan-dev vulkan-tools mesa-vulkan-drivers libgl1-mesa-dri \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb-util-dev \
  libx11-dev libxcursor-dev libxi-dev libxrandr-dev libxrender-dev \
  libgtk-3-dev libasound2-dev libudev-dev libusb-1.0-0-dev libbluetooth-dev \
  libwayland-dev wayland-protocols

echo "==> [2/4] Ensuring a recent Rust toolchain (>= 1.85 for edition2024)"
if command -v rustup >/dev/null 2>&1; then
  rustup toolchain install stable --profile minimal
  rustup default stable
else
  echo "rustup not found; please install a Rust toolchain >= 1.85" >&2
  exit 1
fi
rustc --version

echo "==> [3/4] Building the BrainFlow C++ core + Rust binding libraries"
if [ ! -f "$BRAINFLOW_RUST_LIB/libBoardController.so" ]; then
  sudo mkdir -p "$(dirname "$BRAINFLOW_HOME")"
  sudo chown -R "$(id -u):$(id -g)" /Users
  if [ ! -d "$BRAINFLOW_HOME/.git" ]; then
    git clone https://github.com/brainflow-dev/brainflow.git "$BRAINFLOW_HOME"
  fi
  git -C "$BRAINFLOW_HOME" checkout "$BRAINFLOW_REF" 2>/dev/null || \
    echo "(could not pin BrainFlow to $BRAINFLOW_REF; building current checkout)"

  rm -rf "$BRAINFLOW_HOME/build"
  mkdir -p "$BRAINFLOW_HOME/build"
  cmake -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$BRAINFLOW_HOME/installed" \
    -S "$BRAINFLOW_HOME" -B "$BRAINFLOW_HOME/build"
  cmake --build "$BRAINFLOW_HOME/build" --parallel "$(nproc)"
  cmake --install "$BRAINFLOW_HOME/build"
else
  echo "BrainFlow libraries already present at $BRAINFLOW_RUST_LIB — skipping."
fi
ls -1 "$BRAINFLOW_RUST_LIB"/libBoardController.so \
       "$BRAINFLOW_RUST_LIB"/libDataHandler.so \
       "$BRAINFLOW_RUST_LIB"/libMLModule.so

echo "==> [4/4] Building Mind Harness"
cd "$REPO_ROOT"
cargo build

echo
echo "Setup complete. Build + test with:"
echo "    cargo build && cargo test"
echo
echo "Run the GUI (needs an X display; a headless VM can use software Vulkan):"
echo "    DISPLAY=:1 XDG_RUNTIME_DIR=/tmp/xdg-runtime \\"
echo "      VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \\"
echo "      cargo run --bin mind-harness"
