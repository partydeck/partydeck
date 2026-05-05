use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;

use crate::app::PartyConfig;
use crate::handler::*;
use crate::input::*;
use crate::instance::*;
use crate::paths::*;
use crate::profiles::{create_profile, create_profile_gamesave};
use crate::util::*;

pub fn setup_profiles(
    h: &Handler,
    instances: &Vec<Instance>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n[partydeck] Instances:");
    for instance in instances {
        if instance.profname.starts_with(".") {
            create_profile(&instance.profname)?;
        }
        if h.is_saved_handler() {
            create_profile_gamesave(&instance.profname, h)?;
        }
        println!(
            "[partydeck] - Profile: {}, Monitor: {}, Resolution: {}x{}",
            instance.profname, instance.monitor, instance.width, instance.height
        );
    }
    Ok(())
}

pub fn launch_game(
    h: &Handler,
    input_devices: &[DeviceInfo],
    instances: &Vec<Instance>,
    cfg: &PartyConfig,
    router: &InputRouter,
) -> Result<Vec<std::process::Child>, Box<dyn std::error::Error>> {

    // Restart routing thread to ensure fresh grabs and avoid leaks
    router.stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(20));
    router.stop_signal.store(false, std::sync::atomic::Ordering::Relaxed);

    router.slots.lock().unwrap().clear();

    if h.is_saved_handler() && !cfg.disable_mount_gamedirs {
        fuse_overlayfs_mount_gamedirs(h, instances)?;
    }

    let new_cmds = launch_cmds(h, input_devices, instances, cfg, router)?;

    if cfg.enable_kwin_script {
        let script = if cfg.vertical_two_player { "splitscreen_kwin_vertical.js" } else { "splitscreen_kwin.js" };
        let _ = kwin_dbus_start_script(PATH_RES.join(script));
    }

    let mut handles = Vec::new();
    for (i, mut cmd) in new_cmds.into_iter().enumerate() {
        println!("[partydeck] Spawning Instance {}", i + 1);

        let handle = cmd.spawn().map_err(|e| {
            format!("Failed to start process: {}.", e)
        })?;
        handles.push(handle);

        std::thread::sleep(std::time::Duration::from_millis(1500));
    }

    router.start_routing();
    Ok(handles)
}

pub fn launch_cmds(
    h: &Handler,
    input_devices: &[DeviceInfo],
    instances: &Vec<Instance>,
    cfg: &PartyConfig,
    router: &InputRouter,
) -> Result<Vec<std::process::Command>, Box<dyn std::error::Error>> {
    let win = h.win();
    let exec_path = Path::new(&h.exec);
    let gamescope_bin = if cfg.kbm_support { BIN_GSC_KBM.as_path() } else { Path::new("gamescope") };
    let umu_run = &*BIN_UMU_RUN;

    let mut cmds = Vec::new();

    for (i, instance) in instances.iter().enumerate() {
        let gamedir = if h.is_saved_handler() && !cfg.disable_mount_gamedirs {
            PATH_PARTY.join("tmp").join(format!("game-{}", i))
        } else {
            PathBuf::from(h.get_game_rootpath()?)
        };

        let path_exec = gamedir.join(exec_path);
        let cwd = path_exec.parent().unwrap_or(&gamedir);
        let path_pfx = PATH_PARTY.join("prefixes").join(if cfg.proton_separate_pfxs { (i + 1).to_string() } else { "1".to_string() });
        let path_prof = PATH_PARTY.join("profiles").join(&instance.profname);

        let mut cmd = Command::new(gamescope_bin);
        cmd.current_dir(cwd);

        // Core Environment
        cmd.env("ENABLE_GAMESCOPE_WSI", "0");
        // --- THE CRITICAL INPUT FIX ---
        // 1. Disable Proton's native HID layer that bypasses our virtual nodes
        cmd.env("PROTON_DISABLE_HIDRAW", "1");
        // 2. Disable SDL's HIDAPI which also tries to bypass standard joystick nodes
        cmd.env("SDL_JOYSTICK_HIDAPI", "0");
        
        if win {
            cmd.env("WINEPREFIX", &path_pfx);
            cmd.env("PROTON_VERB", "run");
            cmd.env("PROTONPATH", if cfg.proton_version.is_empty() { "GE-Proton" } else { &cfg.proton_version });

            // 3. Force Wine to use its internal xinput wrapper
            cmd.env("WINEDLLOVERRIDES", "xinput1_3=n,b");
        }

        cmd.args(["-W", &instance.width.to_string(), "-H", &instance.height.to_string()]);
        if cfg.gamescope_sdl_backend {
            cmd.args(["--backend=sdl", &format!("--display-index={}", instance.monitor)]);
        }
        cmd.arg("--");

        // Sandbox
        cmd.arg("bwrap");
        cmd.arg("--die-with-parent");
        cmd.args(["--dev-bind", "/", "/"]);
        cmd.args(["--tmpfs", "/dev/input"]);

        let mut assigned_vnodes = Vec::new();

        for &device_idx in &instance.devices {
            let dev_info = &input_devices[device_idx];
            if dev_info.device_type == DeviceType::Gamepad {
                if let Ok(vnodes) = router.add_slot(&dev_info.path) {
                    for vpath in vnodes {
                        // Use --dev-bind for device nodes to ensure they are created correctly in tmpfs
                        cmd.args(["--dev-bind", &vpath, &vpath]);
                        assigned_vnodes.push(vpath);
                    }
                }
            }
        }

        // Controller Mapping Force
        let evdev_str = assigned_vnodes.iter().filter(|p| p.contains("event")).cloned().collect::<Vec<_>>().join(":");
        if !evdev_str.is_empty() {
            cmd.env("SDL_EVDEV_DEVICES", &evdev_str);
        }

        if h.use_goldberg {
            cmd.env("GseAppPath", PATH_PARTY.join("goldberg_data"));
            cmd.env("GseSavePath", path_prof.join("steam"));
            cmd.env("SteamAppUser", &instance.profname);
            cmd.env("SteamUser", &instance.profname);
            cmd.env("SteamClientLaunch", "1");
            cmd.env("SteamEnv", "1");
        }

        // --- DIAGNOSTIC: Print the command and check sandbox nodes ---
        println!("[partydeck-debug] Instance {} launch command: {:?}", i + 1, cmd);
        
        if win {
            cmd.arg(umu_run);
        }
        cmd.arg(&path_exec);

        for arg in h.args.split_whitespace() {
            cmd.arg(arg);
        }

        cmds.push(cmd);
    }
    Ok(cmds)
}

pub fn fuse_overlayfs_mount_gamedirs(h: &Handler, instances: &Vec<Instance>) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = PATH_PARTY.join("tmp");
    let mut path_lowerdir = h.get_game_rootpath()?;
    let overlay_path = h.path_handler.join("overlay");
    if overlay_path.exists() {
        path_lowerdir = format!("{}:{}", overlay_path.display(), path_lowerdir);
    }
    let gamename = h.handler_dir_name().to_string();
    for (i, instance) in instances.iter().enumerate() {
        let path_game_mnt = tmp_dir.join(format!("game-{}", i));
        let path_workdir = tmp_dir.join(format!("work-{}", i));
        let path_prof = PATH_PARTY.join("profiles").join(&instance.profname);
        let path_upperdir = path_prof.join("gamesaves").join(&gamename);
        let _ = fs::create_dir_all(&path_game_mnt);
        let _ = Command::new("fusermount3").args(["-u", "-z", &path_game_mnt.to_string_lossy()]).status();
        let _ = fs::create_dir_all(&path_workdir);
        let _ = fs::create_dir_all(&path_upperdir);
        let mut cmd = Command::new("fuse-overlayfs");
        cmd.arg("-o")
           .arg(format!("lowerdir={},upperdir={},workdir={}", path_lowerdir, path_upperdir.display(), path_workdir.display()))
           .arg(&path_game_mnt);
        let _ = cmd.status();
    }
    Ok(())
}
