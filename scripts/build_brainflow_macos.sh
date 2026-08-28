#!/usr/bin/env bash
#
# Build BrainFlow C++ core + Rust binding for Apple Silicon (macOS arm64)
#
# This script is part of the OpenBCI GUI Rust native port.
# Run it once before `cargo build` in the Rust project.
#
# Prerequisites:
#   - Xcode + Command Line Tools
#   - CMake (brew install cmake)
#   - Rust toolchain
#
# After running this script successfully, the four key dylibs will be in:
#   brainflow/installed/lib/
#
# Then you can point the Rust brainflow crate at them or copy them into
# the Rust project's resources/ folder for bundling.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Where we expect the brainflow source (sibling to this repo or user-specified)
BRAINFLOW_SRC="${BRAINFLOW_SRC:-$HOME/github/brainflow}"
BUILD_DIR="$BRAINFLOW_SRC/build"
INSTALL_DIR="$BRAINFLOW_SRC/installed"

echo "=============================================================="
echo " BrainFlow macOS arm64 Build Script"
echo "=============================================================="
echo "BrainFlow source : $BRAINFLOW_SRC"
echo "Build directory  : $BUILD_DIR"
echo "Install directory: $INSTALL_DIR"
echo

if [ ! -d "$BRAINFLOW_SRC" ]; then
    echo "ERROR: BrainFlow source not found at $BRAINFLOW_SRC"
    echo "Clone it first:"
    echo "  git clone https://github.com/brainflow-dev/brainflow.git $BRAINFLOW_SRC"
    exit 1
fi

# Clean previous build (optional but recommended for arm64 switch)
rm -rf "$BUILD_DIR" "$INSTALL_DIR"
mkdir -p "$BUILD_DIR" "$INSTALL_DIR"

cd "$BUILD_DIR"

echo ">>> Configuring CMake for macOS arm64 (Release)..."
cmake \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_INSTALL_PREFIX="$INSTALL_DIR" \
    -DBUILD_OPENBCI_GUI=ON \
    -DBUILD_TESTS=OFF \
    -DBUILD_EXAMPLES=OFF \
    "$BRAINFLOW_SRC"

echo
echo ">>> Building (this will take several minutes on first run)..."
cmake --build . --config Release --parallel "$(sysctl -n hw.ncpu)"

echo
echo ">>> Installing to $INSTALL_DIR..."
cmake --install . --config Release

echo
echo ">>> Build complete. Key libraries:"
ls -lh "$INSTALL_DIR/lib/" | grep -E 'lib(BoardController|DataHandler|MLModule|simpleble)' || true

echo
echo "=============================================================="
echo " NEXT STEPS FOR THE RUST GUI"
echo "=============================================================="
echo
echo "1. The Rust brainflow binding needs to find these dylibs."
echo "   Either:"
echo "     a) Add rpath at link time (recommended for app bundle), or"
echo "     b) Copy the four dylibs into openbci-gui-rust/resources/"
echo
echo "2. Then in openbci-gui-rust/, uncomment the brainflow dependency"
echo "   in Cargo.toml and run:"
echo
echo "     cargo build --features generate_binding"
echo
echo "3. The binding will generate brainflow.rs and link the C++ libs."
echo
echo "For more details see the official docs:"
echo "  https://brainflow.readthedocs.io/en/stable/BuildBrainFlow.html#rust"
echo "=============================================================="
