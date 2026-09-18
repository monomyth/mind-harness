#!/usr/bin/env bash
#
# Build BrainFlow C++ core for Apple Silicon (macOS arm64)
#
# Mind Harness expects a sibling BrainFlow checkout:
#   parent/mind-harness  +  parent/brainflow
# parent/brainflow may be a clone or a symlink to one.
#
# Run once before `cargo build` in mind-harness. Uses build/ + installed/
# under the BrainFlow tree (macOS only). For Linux use
# scripts/build_brainflow_linux.sh (separate build-linux/ dirs).
#
# Prerequisites:
#   - Xcode + Command Line Tools
#   - CMake (brew install cmake)
#   - Rust toolchain
#
# After success, key dylibs are under brainflow/installed/lib/. Stage or
# copy them into brainflow/rust_package/brainflow/lib/ (binding default)
# or set BRAINFLOW_LIB to that directory.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Where we expect the brainflow source (sibling to this repo or user-specified)
DEFAULT_SIBLING="$(cd "$PROJECT_ROOT/.." && pwd)/brainflow"
BRAINFLOW_SRC="${BRAINFLOW_SRC:-$DEFAULT_SIBLING}"
# Legacy fallback if sibling missing:
if [ ! -d "$BRAINFLOW_SRC" ] && [ -d "$HOME/github/brainflow" ]; then
  BRAINFLOW_SRC="$HOME/github/brainflow"
fi
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
    echo "Clone it beside mind-harness (or set BRAINFLOW_SRC):"
    echo "  git clone https://github.com/brainflow-dev/brainflow.git $DEFAULT_SIBLING"
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
echo "1. Stage dylibs into the Rust binding lib dir if needed:"
echo "     mkdir -p \"$BRAINFLOW_SRC/rust_package/brainflow/lib\""
echo "     cp \"$INSTALL_DIR\"/lib/libBoardController.dylib \"$BRAINFLOW_SRC/rust_package/brainflow/lib/\""
echo "     cp \"$INSTALL_DIR\"/lib/libDataHandler.dylib \"$BRAINFLOW_SRC/rust_package/brainflow/lib/\""
echo "     cp \"$INSTALL_DIR\"/lib/libMLModule.dylib \"$BRAINFLOW_SRC/rust_package/brainflow/lib/\""
echo
echo "2. From mind-harness beside brainflow:"
echo "     cargo build --release --locked --bin mind-harness"
echo "   Or override: BRAINFLOW_LIB=... cargo build --release --locked"
echo
echo "Docs: https://brainflow.readthedocs.io/en/stable/BuildBrainFlow.html#rust"
echo "=============================================================="
