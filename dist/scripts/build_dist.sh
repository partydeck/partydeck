#!/bin/sh
# Build the release skeleton (on-device layout) into
# dist/build_generated/$BUILD_NAME/release/. Run from the repo root.
set -eu

BUILD_NAME="${BUILD_NAME:-holo}"
BUILD_DIR="${BUILD_DIR:-dist/build_generated/$BUILD_NAME}"
RELEASE_DIR="$BUILD_DIR/release"

# Cargo caches beside the output so a bind-mounted /workspace persists them.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/$BUILD_DIR/target}"
export CARGO_HOME="${CARGO_HOME:-$PWD/$BUILD_DIR/home}"

rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

cargo build --release -F build_gamescope -F download_deps

cp    "$CARGO_TARGET_DIR/release/partydeck" "$RELEASE_DIR/partydeck"
cp -r "$CARGO_TARGET_DIR/release/bin"        "$RELEASE_DIR/bin"
cp -r "$CARGO_TARGET_DIR/release/res"        "$RELEASE_DIR/res"
cp res/GamingModeLauncher.sh "$RELEASE_DIR/GamingModeLauncher.sh"
cp LICENSE                   "$RELEASE_DIR/LICENSE"
cp COPYING.md                "$RELEASE_DIR/thirdparty.txt"

echo "Release skeleton: $RELEASE_DIR"
