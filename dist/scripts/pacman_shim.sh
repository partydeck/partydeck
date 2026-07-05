#!/bin/sh
# pacman wrapper installed at /usr/local/bin/pacman (ahead of /usr/bin in sudo's
# secure_path). build_appimage.sh's get-debloated-pkgs installs current-Arch
# mesa/vulkan packages whose deps name libgcc/libstdc++/vulkan-mesa-implicit-layers
# — provides the SteamOS Holo image's older Arch has but doesn't declare. Mark
# them satisfied; they genuinely are. The flags are inert on other operations.
exec /usr/bin/pacman \
    --assume-installed libgcc \
    --assume-installed libstdc++ \
    --assume-installed vulkan-mesa-implicit-layers \
    "$@"
