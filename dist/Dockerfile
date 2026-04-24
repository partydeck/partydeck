# docker run -v $(pwd)/../:/workspace -w /workspace partydeck-appimage-build
FROM archlinux:latest AS base

RUN pacman-key --init && \
    pacman -Syu --noconfirm && \
    pacman -S --noconfirm git meson rust cargo clang glslang libcap wlroots0.18 sdl2 vulkan-headers libx11 libxmu libxcomposite libxrender libxres libxtst libxkbcommon libdrm libinput wayland-protocols benchmark xorg-xwayland pipewire cmake libavif libheif aom rav1e libdecor libxdamage luajit libva libxss wget curl xorg-server-xvfb fuse-overlayfs bubblewrap libxkbcommon zsync pipewire pulseaudio pipewire-audio alsa-plugins libxkbcommon-x11 libvdpau libvdpau-va-gl sudo pkg-config openssl zip


FROM base AS runtime

# Build args to match host user
ARG UID=1000
ARG GID=1000

# Create matching user
RUN groupadd -g $GID appgroup && \
    useradd -m -u $UID -g $GID appuser && \
    usermod -aG wheel appuser && \
    echo "%wheel ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/wheel && \
    chmod 0440 /etc/sudoers.d/wheel

USER appuser
WORKDIR /workspace

# This currently downloads all binarys from source each time, change CARGO_HOME, the CARGO_TARGET_DIR to a set location passed through with -v if you want to cache.
CMD ["bash", "-c", "export CARGO_TARGET_DIR=$(mktemp -d) && cargo build --release -F build_gamescope -F download_deps && ./dist/build_appimage.sh"]