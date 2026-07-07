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

# PLEASE NOTE: we are using scripts we dont entirely trust. The refs below pin
# them to fixed revisions, but get-debloated-pkgs still fetches packages from
# moving releases internally, so this is not a complete lockdown.
ANYLINUX_REF="b2d7fef33e5c73156ca170bd501251515644cbcf"
ANYLINUX_RAW="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/$ANYLINUX_REF/useful-tools"
APPIMAGETOOL_VERSION="0.3.2"

DEBLOATED_PKGS="$ANYLINUX_RAW/get-debloated-pkgs.sh"
SHARUN="$ANYLINUX_RAW/quick-sharun.sh"
export HOOKSRC="$ANYLINUX_RAW/hooks"
export ANYLINUX_LIB_SOURCE="$ANYLINUX_RAW/lib/anylinux.c"
export GTK_CLASS_FIX_SOURCE="$ANYLINUX_RAW/lib/gtk-class-fix.c"
export APPIMAGETOOL_LINK="https://github.com/pkgforge-dev/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$ARCH-linux"

unset GITHUB_REPOSITORY

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
