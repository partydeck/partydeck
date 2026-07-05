#!/bin/sh
# Build both release artifacts in order: the skeleton, then the AppImage that
# wraps it. Run from the repo root. Honors BUILD_NAME (default: holo).
set -eu

./dist/scripts/build_dist.sh
./dist/scripts/build_appimage.sh
