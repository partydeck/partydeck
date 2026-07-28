#!/bin/sh
# Wrap the release skeleton into a self-contained AppImage via sharun. Run from
# the repo root after build_dist.sh. Override the input with RELEASE_BUNDLE_DIR.
# Output: dist/appimage_generated/partydeck-anylinux-<arch>.AppImage
set -eu

REPO_ROOT="$PWD"
BUILD_NAME="${BUILD_NAME:-holo}"
RELEASE_BUNDLE_DIR="${RELEASE_BUNDLE_DIR:-$REPO_ROOT/dist/build_generated/$BUILD_NAME/release}"
# Accept a repo-relative override.
case "$RELEASE_BUNDLE_DIR" in /*) ;; *) RELEASE_BUNDLE_DIR="$REPO_ROOT/$RELEASE_BUNDLE_DIR" ;; esac

ARCH="$(uname -m)"

# PLEASE NOTE: we are using scripts we dont entirely trust. These can be updated at any time. In the future, we should download and make sure we trust this.
# For now it should be fine, but trusting external updated deps should be avoided.
DEBLOATED_PKGS="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/get-debloated-pkgs.sh"
SHARUN="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/quick-sharun.sh"

export OUTNAME="partydeck-anylinux-$ARCH.AppImage"
export DESKTOP="$REPO_ROOT/dist/assets/partydeck.desktop"
export ICON="$REPO_ROOT/dist/assets/partydeck.png"
export OUTPATH=.
export DEPLOY_SDL=1
export DEPLOY_OPENGL=1
export DEPLOY_VULKAN=1
export STRIP=1

WORK="$REPO_ROOT/dist/appimage_generated"
rm -rf "$WORK"
mkdir -p "$WORK"
cd "$WORK"

wget --retry-connrefused --tries=30 "$DEBLOATED_PKGS" -O ./get-debloated-pkgs
wget --retry-connrefused --tries=30 "$SHARUN"          -O ./quick-sharun
chmod +x ./get-debloated-pkgs ./quick-sharun

./get-debloated-pkgs --add-mesa --add-vulkan

./quick-sharun \
    "$RELEASE_BUNDLE_DIR/partydeck" \
    "$RELEASE_BUNDLE_DIR/bin/gamescope-kbm" \
    "$RELEASE_BUNDLE_DIR/bin/gamescopereaper" \
    "$RELEASE_BUNDLE_DIR/bin/umu-run" \
    /usr/bin/fuse-overlayfs /usr/bin/bwrap /usr/bin/zip

mkdir -p ./AppDir/share/partydeck
cp -r "$RELEASE_BUNDLE_DIR/res/." ./AppDir/share/partydeck/

./quick-sharun --make-appimage

echo "AppImage: $WORK/$OUTNAME"
