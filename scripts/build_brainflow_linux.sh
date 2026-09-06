#!/usr/bin/env bash
#
# Build BrainFlow C++ core + stage libs for the Rust binding (Linux).
#
# Intended layout (sibling checkouts):
#   parent/
#     mind-harness/
#     brainflow/
#
# Verified once on Arch (isengard) for Mind Harness 2.2.48 build path.
# This script does not SSH or touch remote hosts; run it on the Linux box.
#
# Arch packages (see also scripts/install-mh-linux-deps.sh):
#   cmake ninja clang llvm pkgconf python
#   plus GUI/runtime deps listed in install-mh-linux-deps.sh
#
# Never wipes unrelated platform builds: uses build-linux/ and
# installed-linux/ under the BrainFlow source tree.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Prefer sibling ../brainflow; fall back to BRAINFLOW_SRC / legacy default.
DEFAULT_SIBLING="$(cd "$PROJECT_ROOT/.." && pwd)/brainflow"
BRAINFLOW_SRC="${BRAINFLOW_SRC:-$DEFAULT_SIBLING}"
BUILD_DIR="${BRAINFLOW_BUILD_DIR:-$BRAINFLOW_SRC/build-linux}"
INSTALL_DIR="${BRAINFLOW_INSTALL_DIR:-$BRAINFLOW_SRC/installed-linux}"
JOBS="${BRAINFLOW_JOBS:-$(nproc 2>/dev/null || echo 4)}"
RUST_LIB_DIR="$BRAINFLOW_SRC/rust_package/brainflow/lib"

echo "=============================================================="
echo " BrainFlow Linux Build Script"
echo "=============================================================="
echo "BrainFlow source : $BRAINFLOW_SRC"
echo "Build directory  : $BUILD_DIR"
echo "Install directory: $INSTALL_DIR"
echo "Parallel jobs    : $JOBS"
echo

if [ ! -d "$BRAINFLOW_SRC" ]; then
    echo "ERROR: BrainFlow source not found at $BRAINFLOW_SRC"
    echo "Clone beside mind-harness:"
    echo "  git clone https://github.com/brainflow-dev/brainflow.git $DEFAULT_SIBLING"
    exit 1
fi

# MLModule needs onnxruntime_c_api.h even when BUILD_ONNX=OFF (seen on isengard).
ONNX_HDR="$BRAINFLOW_SRC/third_party/onnxruntime/include/onnxruntime_c_api.h"
if [ ! -f "$ONNX_HDR" ]; then
    echo "ERROR: missing $ONNX_HDR"
    echo "Restore third_party/onnxruntime headers from a complete BrainFlow tree"
    echo "before starting a long build (MLModule links against this header)."
    exit 1
fi

mkdir -p "$BUILD_DIR" "$INSTALL_DIR"
cd "$BUILD_DIR"

echo ">>> Configuring CMake for Linux (Release)..."
cmake \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$INSTALL_DIR" \
    -DBUILD_TESTS=OFF \
    -DBUILD_EXAMPLES=OFF \
    -G Ninja \
    "$BRAINFLOW_SRC"

echo
echo ">>> Building with $JOBS jobs..."
cmake --build . --config Release --parallel "$JOBS"

echo
echo ">>> Installing to $INSTALL_DIR..."
cmake --install . --config Release

echo
echo ">>> Staging shared libs into Rust binding lib/ ($RUST_LIB_DIR)..."
mkdir -p "$RUST_LIB_DIR"
# Prefer install tree; fall back to common in-tree locations if needed.
shopt -s nullglob
COPIED=0
for name in libBoardController.so libDataHandler.so libMLModule.so; do
    src=""
    for cand in \
        "$INSTALL_DIR/lib/$name" \
        "$INSTALL_DIR/lib64/$name" \
        "$BUILD_DIR/$name" \
        "$BRAINFLOW_SRC/installed-linux/lib/$name"
    do
        if [ -f "$cand" ]; then
            src="$cand"
            break
        fi
    done
    if [ -z "$src" ]; then
        # last-resort search under install + build dirs only (not whole tree)
        src="$(find "$INSTALL_DIR" "$BUILD_DIR" -maxdepth 4 -name "$name" -type f 2>/dev/null | head -n1 || true)"
    fi
    if [ -n "$src" ] && [ -f "$src" ]; then
        cp -f "$src" "$RUST_LIB_DIR/$name"
        echo "  copied $name <- $src"
        COPIED=$((COPIED + 1))
    else
        echo "  WARNING: $name not found under install/build dirs"
    fi
done
shopt -u nullglob

echo
echo "Key libraries in $RUST_LIB_DIR:"
ls -lh "$RUST_LIB_DIR"/libBoardController.so \
       "$RUST_LIB_DIR"/libDataHandler.so \
       "$RUST_LIB_DIR"/libMLModule.so 2>/dev/null || true

if [ "$COPIED" -lt 3 ]; then
    echo "ERROR: expected three .so files in $RUST_LIB_DIR"
    exit 1
fi

echo
echo "=============================================================="
echo " NEXT: from mind-harness/"
echo "   cargo build --release --locked --bin mind-harness"
echo " Optional override:"
echo "   BRAINFLOW_LIB=$RUST_LIB_DIR cargo build --release --locked"
echo "=============================================================="
