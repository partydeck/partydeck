#!/bin/sh

# You might need to restart your pc if sharun doesn't create `AppDir` in this directory (It should create dirs on its own)

# Grab release from https://github.com/wunnr/partydeck/releases and extract it to the same dir as this .sh file
set -eu

cd dist;

rm -rf appimage_generated
mkdir appimage_generated
cd appimage_generated

ARCH="$(uname -m)"
DEBLOATED_PKGS="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/get-debloated-pkgs.sh"
SHARUN="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/quick-sharun.sh"

export ADD_HOOKS="self-updater.bg.hook"
#export UPINFO="gh-releases-zsync|${GITHUB_REPOSITORY%/*}|${GITHUB_REPOSITORY#*/}|latest|*$ARCH.AppImage.zsync"
export OUTNAME=partydeck-anylinux-"$ARCH".AppImage
export DESKTOP=../partydeck.desktop
export ICON=../partydeck.png
export OUTPATH=.
export DEPLOY_SDL=1
export DEPLOY_OPENGL=1
export DEPLOY_VULKAN=1
export STRIP=1

: ${CARGO_TARGET_DIR:=../../target}

# ADD LIBRARIES
wget --retry-connrefused --tries=30 "$DEBLOATED_PKGS" -O ./get-debloated-pkgs
wget --retry-connrefused --tries=30 "$SHARUN" -O ./quick-sharun
chmod +x ./get-debloated-pkgs
chmod +x ./quick-sharun

# Debloated pkgs
./get-debloated-pkgs --add-mesa --add-vulkan

# Point to binaries
./quick-sharun $CARGO_TARGET_DIR/release/partydeck $CARGO_TARGET_DIR/release/bin/gamescope-kbm $CARGO_TARGET_DIR/release/bin/gamescopereaper $CARGO_TARGET_DIR/release/bin/umu-run /usr/bin/fuse-overlayfs /usr/bin/bwrap /usr/bin/zip

# Res
mkdir -p ./AppDir/share/partydeck
cp -r $CARGO_TARGET_DIR/release/res/* ./AppDir/share/partydeck

# Make AppImage
./quick-sharun --make-appimage

echo "All Done!"