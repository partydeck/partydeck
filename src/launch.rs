use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::{PartyConfig, PadFilterType};
use crate::handler::*;
use crate::input::*;
use crate::instance::*;
use crate::paths::*;
use crate::profiles::{create_profile, create_profile_gamesave, read_profile_eos_username};
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
) -> Result<(), Box<dyn std::error::Error>> {
    let new_cmds = launch_cmds(h, input_devices, instances, cfg)?;
    print_launch_cmds(&new_cmds);

    if cfg.enable_kwin_script {
        let script = match cfg.vertical_two_player {
            true => "splitscreen_kwin_vertical.js",
            false => "splitscreen_kwin.js",
        };

        kwin_dbus_start_script(PATH_RES.join(script)).map_err(|e| format!("Failed to start KWin script: {}", e))?;
    }

    let sleep_time = match h.pause_between_starts {
        Some(f) => f,
        None => 0.5,
    };

    let mut handles = Vec::new();

    let mut i = 0;
    for mut cmd in new_cmds {
        let handle = cmd.spawn().map_err(|e| {
            format!("Failed to start '{}': {}", cmd.get_program().to_string_lossy(), e)
        })?;
        handles.push(handle);

        if i < instances.len() - 1 {
            std::thread::sleep(std::time::Duration::from_secs_f64(sleep_time));
        }
        i += 1;
    }

    for mut handle in handles {
        handle.wait()?;
    }

    Ok(())
}

pub fn launch_cmds(
    h: &Handler,
    input_devices: &[DeviceInfo],
    instances: &Vec<Instance>,
    cfg: &PartyConfig,
) -> Result<Vec<std::process::Command>, Box<dyn std::error::Error>> {
    let win = h.win();
    let exec = Path::new(&h.exec);
    let runtime = h.runtime.as_str();
    let gamescope = match cfg.kbm_support {
        true => BIN_GSC_KBM.as_path(),
        false => Path::new("gamescope"),
    };

    if cfg.kbm_support && !gamescope.exists() {
        return Err("gamescope-kbm is missing. Please reinstall partydeck or disable KBM support.".into());
    }

    if !cfg.kbm_support && pathsearch::find_executable_in_path("gamescope").is_none() {
        return Err("gamescope not found in PATH. Please install gamescope through your distro's package manager.".into());
    }

    if (runtime == "scout" && !PATH_STEAM.join("bin32/steam-runtime/run.sh").exists())
        || (runtime == "soldier"
            && !PATH_STEAM
                .join("steam/steamapps/common/SteamLinuxRuntime_soldier")
                .exists())
        || (runtime == "sniper"
            && !PATH_STEAM.join("steam/steamapps/common/SteamLinuxRuntime_sniper").exists()
            && !PATH_STEAM.join("steam/steamapps/common/SteamLinuxRuntime_sniper-arm64").exists())
        || (runtime == "steamrt4"
            && !PATH_STEAM.join("steam/steamapps/common/SteamLinuxRuntime_4").exists())
    {
        return Err(format!("Steam Runtime {runtime} not found! Runtime must be installed on the same drive that the Steam client is installed on.").into());
    }

    let mut cmds: Vec<Command> = (0..instances.len())
        .map(|_| Command::new(gamescope))
        .collect();

    for (i, instance) in instances.iter().enumerate() {
        let gamedir = if h.is_saved_handler() && !cfg.disable_mount_gamedirs && cfg.profile_unique_dirs {
            PATH_PARTY.join("tmp").join(format!("game-{}", i))
        } else {
            PathBuf::from(h.get_game_rootpath()?)
        };

        if !gamedir.join(exec).exists() {
            return Err(format!("Executable not found: {}", gamedir.join(exec).display()).into());
        }

        let path_exec = gamedir.join(exec);
        let cwd = path_exec.parent().ok_or_else(|| "couldn't get parent")?;

        let path_prof = PATH_PARTY.join("profiles").join(&instance.profname);
        let path_pfx = PATH_PARTY
            .join("prefixes")
            .join(match cfg.proton_separate_pfxs {
                true => (i + 1).to_string(),
                false => "1".to_string(),
            });

        let cmd = &mut cmds[i];

        cmd.current_dir(cwd);

        if !win || !h.enable_hidraw {
            cmd.env("SDL_JOYSTICK_HIDAPI", "0");
        }
        cmd.env("ENABLE_GAMESCOPE_WSI", "0");
        if h.sdl2_override != SDL2Override::No {
            let path_sdl = match h.sdl2_override {
                SDL2Override::Srt => {
                    PATH_STEAM.join("bin32/steam-runtime/usr/lib/i386-linux-gnu/libSDL2-2.0.so.0")
                }
                SDL2Override::Sys => PathBuf::from("/usr/lib/libSDL2.so"),
                _ => PathBuf::new(),
            };
            cmd.env("SDL_DYNAMIC_API", path_sdl);
        }
        if win {
            let protonpath = match cfg.proton_version.is_empty() {
                true => "GE-Proton",
                false => &cfg.proton_version,
            };

            cmd.env("WINEPREFIX", &path_pfx);
            cmd.env("PROTON_VERB", "run");
            cmd.env("PROTONPATH", protonpath);
            if h.enable_hidraw {
                cmd.env("PROTON_ENABLE_HIDRAW", "1");
            } else {
                cmd.env("PROTON_DISABLE_HIDRAW", "1");
            }
            if cfg.proton_wow64 {
                cmd.env("PROTON_USE_WOW64", "1");
            }
        }
        if cfg.pad_filter_type != PadFilterType::NoSteamInput {
            cmd.env("SDL_GAMECONTROLLER_ALLOW_STEAM_VIRTUAL_GAMEPAD", "1");
        }
        if cfg.pad_filter_type == PadFilterType::OnlySteamInput {
            cmd.env("SDL_GAMECONTROLLER_IGNORE_DEVICES", SDL_GAMECONTROLLER_IGNORE_DEVICES);
        }
        let mut dll_overrides: Vec<String> = Vec::new();
        if !h.env.is_empty() {
            for env_var in h.env.split_whitespace() {
                if let Some((key, value)) = env_var.split_once('=') {
                    // WINEDLLOVERRIDES is set below so the EOS override can be
                    // merged in without clobbering the handler's own entries.
                    if key == "WINEDLLOVERRIDES" {
                        dll_overrides.push(value.to_string());
                    } else {
                        cmd.env(key, value);
                    }
                }
            }
        }
        // For EOS games, make Wine load the Nemirtingas emulator DLL as native
        // automatically (merged with any overrides the handler set), so enabling
        // EOS in the editor is all that's needed — no hand-edited env string.
        if win && h.eos.enabled && !dll_overrides.iter().any(|o| o.contains("EOSSDK-Win64-Shipping"))
        {
            dll_overrides.push("EOSSDK-Win64-Shipping=n,b".to_string());
        }
        if !dll_overrides.is_empty() {
            cmd.env("WINEDLLOVERRIDES", dll_overrides.join(";"));
        }

        // Gamescope args
        if h.use_mangohud {
            cmd.arg("--mangoapp");
        }
        cmd.args([
            "-W",
            &instance.width.to_string(),
            "-H",
            &instance.height.to_string(),
        ]);
        if cfg.gamescope_force_grab_cursor {
            cmd.arg("--force-grab-cursor");
        }
        if cfg.gamescope_sdl_backend {
            cmd.arg("--backend=sdl");
            cmd.arg(format!("--display-index={}", instance.monitor));
        }
        if cfg.kbm_support {
            let mut instance_has_keyboard = false;
            let mut instance_has_mouse = false;
            let mut kbms = String::new();

            for &d in &instance.devices {
                let dev = &input_devices[d];
                if dev.device_type == DeviceType::Keyboard {
                    instance_has_keyboard = true;
                } else if dev.device_type == DeviceType::Mouse {
                    instance_has_mouse = true;
                }
                if dev.device_type == DeviceType::Keyboard || dev.device_type == DeviceType::Mouse {
                    kbms.push_str(&format!("{},", &dev.path));
                }
            }

            if instance_has_keyboard {
                cmd.arg("--backend-disable-keyboard");
            }
            if instance_has_mouse {
                cmd.arg("--backend-disable-mouse");
            }
            if !kbms.is_empty() {
                cmd.arg(format!("--libinput-hold-dev={}", kbms));
                cmd.arg("--grab");
            }
        }
        cmd.arg("--");

        // Bwrap args
        cmd.arg("bwrap");
        cmd.arg("--die-with-parent");
        cmd.args(["--dev-bind", "/", "/"]);
        cmd.args(["--tmpfs", "/tmp"]);
        // Mask out any gamepads that aren't this player's
        for (d, dev) in input_devices.iter().enumerate() {
            if !dev.enabled
                || (!instance.devices.contains(&d) && dev.device_type == DeviceType::Gamepad)
            {
                cmd.args(["--bind", "/dev/null", &dev.path]);
                // Wine's winebus reads controllers via /dev/hidraw* when
                // hidraw is exposed, so masking only the evdev node leaks
                // input to every instance.
                if h.enable_hidraw {
                    for hp in &dev.hidraw_paths {
                        cmd.args(["--bind", "/dev/null", hp]);
                    }
                }
            }
        }

        if cfg.profile_unique_dirs {
            if win {
                let path_pfx_user = path_pfx.join("drive_c/users/steamuser");
                cmd.arg("--bind")
                    .args([&path_prof.join("windata"), &path_pfx_user]);
            } else {
                let path_prof_home = path_prof.join("home");
                cmd.env("HOME", &path_prof_home);
                // Also bind the Steam directory as the Steam runtimes look for HOME/.steam
                if !runtime.is_empty() || h.steam_appid.is_some() {
                    cmd.args([
                        "--bind",
                        &PATH_STEAM.to_string_lossy(),
                        &path_prof_home.join(".steam").to_string_lossy(),
                    ]);
                }
            }
        }

        for subpath in &h.game_null_paths {
            let game_subpath = gamedir.join(subpath);
            if game_subpath.is_file() {
                cmd.args(["--bind", "/dev/null", &game_subpath.to_string_lossy()]);
            } else if game_subpath.is_dir() {
                cmd.args([
                    "--bind",
                    &PATH_PARTY.join("tmp/null").to_string_lossy(),
                    &game_subpath.to_string_lossy(),
                ]);
            }
        }

        let is_appimage = std::env::var("APPIMAGE").is_ok();
        if is_appimage {
            // Because we are faking temp directory, this makes the system use the real vulkan directory for games
            // Used here because the env var is set durring bwrap and gamescope process starting so env cant be cleared at this stage.
            cmd.args(["--unsetenv","VK_DRIVER_FILES"]); 
        }

        if h.use_goldberg {
            cmd.env("GseAppPath", PATH_PARTY.join("goldberg_data"));
            cmd.env("GseSavePath", path_prof.join("steam"));
            cmd.env("SteamAppUser", instance.profname.clone());
            cmd.env("SteamUser", instance.profname.clone());
            cmd.env("SteamClientLaunch", "1");
            cmd.env("SteamEnv", "1");
            if let Some(appid) = h.steam_appid {
                cmd.env("SteamAppId", &appid.to_string());
                cmd.env("SteamGameId", &appid.to_string());
            }

            let sdk32_link = std::fs::read_link(PATH_STEAM.join("sdk32")).map_err(|e| format!("Failed to read sdk32 link: {}", e))?;
            let sdk64_link = std::fs::read_link(PATH_STEAM.join("sdk64")).map_err(|e| format!("Failed to read sdk64 link: {}", e))?;

            cmd.arg("--bind").args([
                PATH_RES.join("goldberg/linux32"),
                sdk32_link,
            ]);

            cmd.arg("--bind").args([
                PATH_RES.join("goldberg/linux64"),
                sdk64_link,
            ]);

            if win {
                cmd.arg("--bind").args([
                    PATH_RES.join("goldberg/win"),
                    path_pfx.join("drive_c/Program Files (x86)/Steam"),
                ]);
            }
        }

        // Runtime
        if win {
            cmd.arg(&*BIN_UMU_RUN);
        } else {
            match runtime {
                "scout" => {
                    cmd.arg(PATH_STEAM.join("bin32/steam-runtime/run.sh"));
                }
                "soldier" => {
                    cmd.arg(
                        PATH_STEAM.join(
                            "steam/steamapps/common/SteamLinuxRuntime_soldier/_v2-entry-point",
                        ),
                    );
                    cmd.arg("--");
                }
                "sniper" => {
                    let sniper_path = PATH_STEAM.join(
                        "steam/steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point",
                    );
                    // old installations of sniper go in a folder named -arm64 even though it is x86_64?
                    let sniper_arm_path = PATH_STEAM.join(
                        "steam/steamapps/common/SteamLinuxRuntime_sniper-arm64/_v2-entry-point",
                    );
                    if sniper_path.exists() {
                        cmd.arg(sniper_path);
                    } else if sniper_arm_path.exists() {
                        cmd.arg(sniper_arm_path);
                    }
                    cmd.arg("--");
                }
                "steamrt4" => {
                    cmd.arg(
                        PATH_STEAM.join(
                            "steam/steamapps/common/SteamLinuxRuntime_4/_v2-entry-point",
                        ),
                    );
                    cmd.arg("--");
                }
                _ => {}
            };
        }


        cmd.arg(&path_exec);

        for arg in h.args.split_whitespace() {
            let processed_arg = match arg {
                "$PROFILE" => &instance.profname,
                "$WIDTH" => &instance.width.to_string(),
                "$HEIGHT" => &instance.height.to_string(),
                "$RESOLUTION" => &format!("{}x{}", instance.width, instance.height),
                "$INSTANCECOUNT" => &instances.len().to_string(),
                "$INSTANCENUM" => &i.to_string(),
                "$GAMEDIR" => &gamedir.os_fmt(win),
                "$HANDLERDIR" => &h.path_handler.os_fmt(win),
                _ => &String::from(arg).sanitize_path(),
            };
            cmd.arg(processed_arg);
        }
    }

    Ok(cmds)
}

fn print_launch_cmds(cmds: &Vec<Command>) {
    for (i, cmd) in cmds.iter().enumerate() {
        println!("[partydeck] INSTANCE {}:", i + 1);

        let cwd = cmd.get_current_dir().unwrap_or_else(|| Path::new(""));
        println!("[partydeck] CWD={}", cwd.display());

        for var in cmd.get_envs() {
            let value = var.1.ok_or_else(|| "").unwrap_or_default();
            println!(
                "[partydeck] {}={}",
                var.0.to_string_lossy(),
                value.display()
            );
        }

        println!("[partydeck] \"{}\"", cmd.get_program().display());

        print!("[partydeck] ");
        for arg in cmd.get_args() {
            let fmtarg = arg.to_string_lossy();
            if fmtarg == "--bind"
                || fmtarg == "bwrap"
                || (fmtarg.starts_with("/") && fmtarg.len() > 1)
            {
                print!("\n[partydeck] ");
            } else {
                print!(" ");
            }
            print!("\"{}\"", fmtarg);
        }

        println!("\n[partydeck] ---------------------");
    }
}

pub fn fuse_overlayfs_mount_gamedirs(
    h: &Handler,
    instances: &Vec<Instance>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = PATH_PARTY.join("tmp");
    let mut path_lowerdir = h.get_game_rootpath()?;

    let overlay_path = h.path_handler.join("overlay");
    if overlay_path.exists() {
        path_lowerdir = format!("{}:{}", overlay_path.display(), path_lowerdir);
    }

    let gamename = h.handler_dir_name().to_string();

    let mut cmds: Vec<Command> = (0..instances.len())
        .map(|_| Command::new("fuse-overlayfs"))
        .collect();

    for (i, instance) in instances.iter().enumerate() {
        let cmd = &mut cmds[i];

        let path_game_mnt = tmp_dir.join(format!("game-{}", i));
        let path_workdir = tmp_dir.join(format!("work-{}", i));
        let path_prof = PATH_PARTY.join("profiles").join(&instance.profname);
        let path_upperdir = path_prof.join("gamesaves").join(&gamename);

        std::fs::create_dir_all(&path_game_mnt)?;
        std::fs::create_dir_all(&path_workdir)?;

        cmd.arg("-o");
        cmd.arg(format!("lowerdir={}", path_lowerdir));
        cmd.arg("-o");
        cmd.arg(format!("upperdir={}", path_upperdir.display()));
        cmd.arg("-o");
        cmd.arg(format!("workdir={}", path_workdir.display()));
        cmd.arg(&path_game_mnt);
    }

    for cmd in &mut cmds {
        let status = cmd
            .status()
            .map_err(|_| "Fuse-overlayfs executable not found; Please install fuse-overlayfs through your distro's package manager. If you already have it installed (or are on SteamOS, where it should be pre-installed), open up an issue on the GitHub.")?;
        if !status.success() {
            return Err("fuse-overlayfs mount failed.".into());
        }
    }

    Ok(())
}

/// A valid 32-hex-char EOS account id, unique per (profile name, instance slot).
/// Two simultaneously-running instances are guaranteed distinct ids (the slot
/// index is folded in), which is what the lobby needs.
fn eos_account_id(profname: &str, idx: usize) -> String {
    use std::hash::{Hash, Hasher};
    let mk = |salt: u64| -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        salt.hash(&mut h);
        profname.hash(&mut h);
        (idx as u64).hash(&mut h);
        h.finish()
    };
    format!("{:016x}{:016x}", mk(0x9e37_79b9_7f4a_7c15), mk(0xc2b2_ae3d_27d4_eb4f))
}

/// A second 32-hex id (EOS ProductUserId), distinct from the EpicId but stable
/// per (profile, slot). The emulator keys identity off both.
fn eos_product_id(profname: &str, idx: usize) -> String {
    use std::hash::{Hash, Hasher};
    let mk = |salt: u64| -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        salt.hash(&mut h);
        "product".hash(&mut h);
        profname.hash(&mut h);
        (idx as u64).hash(&mut h);
        h.finish()
    };
    format!("{:016x}{:016x}", mk(0x2545_f491_4f6c_dd1d), mk(0x1d8e_4e27_c47d_124f))
}

/// Build a full Nemirtingas EOS emulator config (`NemirtingasEpicEmu.json`) from
/// a handler's [`EosConfig`] plus one instance's identity. This is the *nested*
/// schema the current Nemirtingas emulator actually reads
/// (`EOSEmu.User.UserName`/`EpicId`/`ProductUserId`, `Network.Plugins.Broadcast`)
/// — writing a distinct identity per instance here is what lets two instances
/// see each other's lobbies instead of filtering them out as "self". Pure, so
/// it's unit-tested below.
fn build_eos_config(
    eos: &EosConfig,
    username: &str,
    epicid: &str,
    productid: &str,
) -> serde_json::Value {
    let trace = eos.log_level.eq_ignore_ascii_case("trace");
    serde_json::json!({
        "EOSEmu": {
            "Application": {
                "AppId": "InvalidAppId",
                "DisableCrashDump": false,
                "DisableOnlineNetworking": eos.disable_online_networking,
                "LogLevel": eos.log_level,
                "SavePath": "appdata"
            },
            "Ecom": { "UnlockDlcs": eos.unlock_dlcs },
            "Plugins": { "Overlay": { "DelayDetection": "5s", "Enabled": eos.enable_overlay } },
            "User": {
                "Language": eos.language,
                "UserName": username,
                "EpicId": epicid,
                "ProductUserId": productid
            }
        },
        "Network": {
            "IceServers": [],
            "Plugins": {
                "Broadcast": {
                    "EnableLog": trace,
                    "Enabled": eos.broadcast_enabled,
                    "LocalhostOnly": eos.broadcast_localhost_only
                },
                "WebSocket": { "EnableLog": false, "SignalingServers": [] }
            }
        }
    })
}

/// For EOS games, write a per-instance `NemirtingasEpicEmu.json` into each
/// mounted game dir. PartyDeck shares one overlay across every instance (unlike
/// Nucleus, which copies the game per player), so without this every instance
/// would share one EOS identity and the emulator would hide the others' lobbies
/// as its own. The config is generated from the handler's [`EosConfig`] (so it's
/// editable from the UI, no hand-edited JSON) with a unique identity per
/// instance, the display name inherited from the profile (overridable per
/// profile). No-op unless `eos.enabled`. Must run after
/// `fuse_overlayfs_mount_gamedirs`.
pub fn apply_epic_emu_identities(
    h: &Handler,
    instances: &Vec<Instance>,
) -> Result<(), Box<dyn std::error::Error>> {
    apply_epic_emu_identities_to(h, instances, &PATH_PARTY.join("tmp"))
}

/// Inner form taking the mount base dir explicitly (per-instance dirs are
/// `<tmp_dir>/game-<i>`), so the full behaviour is unit-testable without the
/// global `PATH_PARTY`.
fn apply_epic_emu_identities_to(
    h: &Handler,
    instances: &Vec<Instance>,
    tmp_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !h.eos.enabled {
        return Ok(());
    }

    // Where the emulator loads its config from, relative to each game mount: the
    // canonical spot is next to the executable (where the emu DLL lives), and
    // the emulator reads a `nepice_settings/` copy too. Also overwrite any
    // `NemirtingasEpicEmu.json` the handler already ships in its overlay so a
    // stale hand-authored identity can't linger. Deduplicated.
    let mut rel_paths: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        if !rel_paths.contains(&p) {
            rel_paths.push(p);
        }
    };
    if let Some(exedir) = Path::new(&h.exec).parent() {
        add(exedir.join("NemirtingasEpicEmu.json"));
        add(exedir.join("nepice_settings/NemirtingasEpicEmu.json"));
    }
    for rel in find_files_named(&h.path_handler.join("overlay"), "NemirtingasEpicEmu.json") {
        add(rel);
    }
    if rel_paths.is_empty() {
        return Ok(());
    }

    for (i, instance) in instances.iter().enumerate() {
        let game_mnt = tmp_dir.join(format!("game-{}", i));
        // EOS display name inherits the profile's display name (guest '.' dot
        // stripped, e.g. ".Yokatta" -> "Yokatta"), unless the profile pins its
        // own EOS-name override.
        let stripped = instance.profname.trim_start_matches('.').to_string();
        let username = read_profile_eos_username(&instance.profname).unwrap_or(stripped);
        let epicid = eos_account_id(&instance.profname, i);
        let productid = eos_product_id(&instance.profname, i);
        let cfg = build_eos_config(&h.eos, &username, &epicid, &productid);
        let body = serde_json::to_string_pretty(&cfg)?;

        for rel in &rel_paths {
            let target = game_mnt.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, &body)?;
        }
        println!(
            "[partydeck] EOS identity (instance {}): username='{}' epicid={}",
            i + 1,
            username,
            epicid
        );
    }
    Ok(())
}

const SDL_GAMECONTROLLER_IGNORE_DEVICES: &str = "0x054c/0x0df2,0x054c/0x0df2,0x045e/0x02e3,0x045e/0x0b00,0x045e/0x0b05,0x2dc8/0x6000,0x2dc8/0x6100,0x2dc8/0x6001,0x2dc8/0x6101,0x2dc8/0x6003,0x2dc8/0x6006,0x2dc8/0x6009,0x2dc8/0x6012,0x28de/0x1002,0x28de/0x1003,0x28de/0x1071,0x28de/0x1052,0x28de/0x1042,0x28de/0x1203,0x28de/0x1204,0x28de/0x1205,0x28de/0x1206,0x28de/0x1302,0x28de/0x1303,0x28de/0x1304,0x28de/0x1305,0x0f0d/0x01ab,0x0f0d/0x0196,0x28de/0x12ff,0x28de/0x12fe,0x28de/0x12fd,0x28de/0x12fc,0x28de/0x12fb,0x28de/0x12fa,0x28de/0x12f9,0x28de/0x12f8,0x28de/0x12f7,0x28de/0x12f6,0x28de/0x12f5,0x28de/0x12f4,0x28de/0x12f3,0x28de/0x12f2,0x28de/0x12f1,0x28de/0x12f0,0x0079/0x181a,0x044f/0xb315,0x044f/0xd007,0x046d/0xcad1,0x054c/0x0268,0x056e/0x200f,0x056e/0x2013,0x05b8/0x1004,0x05b8/0x1006,0x06a3/0xf622,0x0738/0x3180,0x0738/0x3250,0x0738/0x3481,0x0738/0x8180,0x0738/0x8838,0x0810/0x0001,0x0810/0x0003,0x0925/0x0005,0x0925/0x8866,0x0925/0x8888,0x0e6f/0x0109,0x0e6f/0x011e,0x0e6f/0x0128,0x0e6f/0x0214,0x0e6f/0x1314,0x0e6f/0x6302,0x0e8f/0x0008,0x0e8f/0x3075,0x0e8f/0x310d,0x0f0d/0x0009,0x0f0d/0x004d,0x0f0d/0x005f,0x0f0d/0x006a,0x0f0d/0x006e,0x0f0d/0x0085,0x0f0d/0x0086,0x0f0d/0x0088,0x0f30/0x1100,0x11ff/0x3331,0x1345/0x1000,0x1345/0x6005,0x146b/0x5500,0x1a34/0x0836,0x20bc/0x5500,0x20d6/0x576d,0x20d6/0xca6d,0x2563/0x0523,0x2563/0x0575,0x25f0/0x83c3,0x25f0/0xc121,0x2c22/0x2003,0x2c22/0x2302,0x2c22/0x2502,0x8380/0x0003,0x8888/0x0308,0x0079/0x181b,0x044f/0xd00e,0x054c/0x05c4,0x054c/0x05c5,0x054c/0x09cc,0x054c/0x0ba0,0x0738/0x8250,0x0738/0x8384,0x0738/0x8480,0x0738/0x8481,0x0c12/0x0e10,0x0c12/0x0e13,0x0c12/0x0e15,0x0c12/0x0e20,0x0c12/0x0ef6,0x0c12/0x1cf6,0x0c12/0x1e10,0x0c12/0x2e18,0x0e6f/0x0203,0x0e6f/0x0207,0x0e6f/0x020a,0x0f0d/0x0055,0x0f0d/0x005e,0x0f0d/0x0066,0x0f0d/0x0084,0x0f0d/0x0087,0x0f0d/0x008a,0x0f0d/0x009c,0x0f0d/0x00a0,0x0f0d/0x00ee,0x0f0d/0x011c,0x0f0d/0x0123,0x0f0d/0x0162,0x11c0/0x4001,0x146b/0x0d01,0x146b/0x0d02,0x146b/0x0d06,0x146b/0x0d08,0x146b/0x0d09,0x146b/0x0d10,0x146b/0x0d10,0x146b/0x0d13,0x146b/0x1103,0x1532/0x0401,0x1532/0x1000,0x1532/0x1004,0x1532/0x1007,0x1532/0x1008,0x1532/0x1009,0x1532/0x100a,0x1532/0x1100,0x20d6/0x792a,0x2c22/0x2000,0x2c22/0x2300,0x2c22/0x2500,0x3285/0x0d16,0x3285/0x0d17,0x7545/0x0104,0x9886/0x0025,0x054c/0x0ce6,0x054c/0x0df2,0x054c/0x0e5f,0x0e6f/0x0209,0x0f0d/0x0163,0x0f0d/0x0184,0x1532/0x100b,0x1532/0x100c,0x1532/0x1012,0x3285/0x0d18,0x3285/0x0d19,0x358a/0x0104,0x0079/0x18d4,0x03eb/0xff02,0x044f/0xb326,0x045e/0x028e,0x045e/0x028f,0x045e/0x0291,0x045e/0x02a0,0x045e/0x02a1,0x045e/0x02a9,0x045e/0x0719,0x046d/0xc21d,0x046d/0xc21e,0x046d/0xc21f,0x046d/0xc242,0x056e/0x2004,0x0738/0x4716,0x0738/0x4718,0x0738/0x4726,0x0738/0x4728,0x0738/0x4736,0x0738/0x4738,0x0738/0x4740,0x0738/0xb726,0x0738/0xbeef,0x0738/0xcb02,0x0738/0xcb03,0x0738/0xf738,0x0955/0x7210,0x0955/0xb400,0x0b05/0x1b4c,0x0e6f/0x0105,0x0e6f/0x0113,0x0e6f/0x011f,0x0e6f/0x0125,0x0e6f/0x0127,0x0e6f/0x0131,0x0e6f/0x0133,0x0e6f/0x0143,0x0e6f/0x0147,0x0e6f/0x0201,0x0e6f/0x0213,0x0e6f/0x021f,0x0e6f/0x0301,0x0e6f/0x0313,0x0e6f/0x0314,0x0e6f/0x0401,0x0e6f/0x0413,0x0e6f/0x0501,0x0e6f/0xf900,0x0f0d/0x000a,0x0f0d/0x000c,0x0f0d/0x000d,0x0f0d/0x0016,0x0f0d/0x001b,0x0f0d/0x008c,0x0f0d/0x00db,0x0f0d/0x011e,0x1038/0x1430,0x1038/0x1431,0x1038/0xb360,0x11c9/0x55f0,0x12ab/0x0004,0x12ab/0x0301,0x12ab/0x0303,0x1430/0x02a0,0x1430/0x4748,0x1430/0xf801,0x146b/0x0601,0x15e4/0x3f00,0x15e4/0x3f0a,0x15e4/0x3f10,0x162e/0xbeef,0x1689/0xfd00,0x1689/0xfd01,0x1689/0xfe00,0x1949/0x041a,0x1bad/0x0002,0x1bad/0x0003,0x1bad/0xf016,0x1bad/0xf018,0x1bad/0xf019,0x1bad/0xf021,0x1bad/0xf023,0x1bad/0xf025,0x1bad/0xf027,0x1bad/0xf028,0x1bad/0xf02e,0x1bad/0xf036,0x1bad/0xf038,0x1bad/0xf039,0x1bad/0xf03a,0x1bad/0xf03d,0x1bad/0xf03e,0x1bad/0xf03f,0x1bad/0xf042,0x1bad/0xf080,0x1bad/0xf501,0x1bad/0xf502,0x1bad/0xf503,0x1bad/0xf504,0x1bad/0xf505,0x1bad/0xf506,0x1bad/0xf900,0x1bad/0xf901,0x1bad/0xf902,0x1bad/0xf903,0x1bad/0xf904,0x1bad/0xf906,0x1bad/0xfa01,0x1bad/0xfd00,0x1bad/0xfd01,0x24c6/0x5000,0x24c6/0x5300,0x24c6/0x5303,0x24c6/0x530a,0x24c6/0x531a,0x24c6/0x5397,0x24c6/0x5500,0x24c6/0x5501,0x24c6/0x5502,0x24c6/0x5503,0x24c6/0x5506,0x24c6/0x550d,0x24c6/0x550e,0x24c6/0x5508,0x24c6/0x5510,0x24c6/0x5b00,0x24c6/0x5b02,0x24c6/0x5b03,0x24c6/0x5d04,0x24c6/0xfafa,0x24c6/0xfafb,0x24c6/0xfafc,0x24c6/0xfafd,0x24c6/0xfafe,0x03f0/0x0495,0x044f/0xd012,0x045e/0x02d1,0x045e/0x02dd,0x045e/0x02e0,0x045e/0x02e3,0x045e/0x02ea,0x045e/0x02fd,0x045e/0x02ff,0x045e/0x0b00,0x045e/0x0b05,0x045e/0x0b0a,0x045e/0x0b0c,0x045e/0x0b12,0x045e/0x0b13,0x045e/0x0b20,0x045e/0x0b21,0x045e/0x0b22,0x0738/0x4a01,0x0e6f/0x0139,0x0e6f/0x013b,0x0e6f/0x013a,0x0e6f/0x0145,0x0e6f/0x0146,0x0e6f/0x015b,0x0e6f/0x015c,0x0e6f/0x015d,0x0e6f/0x015f,0x0e6f/0x0160,0x0e6f/0x0161,0x0e6f/0x0162,0x0e6f/0x0163,0x0e6f/0x0164,0x0e6f/0x0165,0x0e6f/0x0166,0x0e6f/0x0167,0x0e6f/0x0205,0x0e6f/0x0206,0x0e6f/0x0246,0x0e6f/0x0261,0x0e6f/0x0262,0x0e6f/0x02a0,0x0e6f/0x02a1,0x0e6f/0x02a2,0x0e6f/0x02a3,0x0e6f/0x02a4,0x0e6f/0x02a5,0x0e6f/0x02a6,0x0e6f/0x02a7,0x0e6f/0x02a8,0x0e6f/0x02a9,0x0e6f/0x02aa,0x0e6f/0x02ab,0x0e6f/0x02ac,0x0e6f/0x02ad,0x0e6f/0x02ae,0x0e6f/0x02af,0x0e6f/0x02b0,0x0e6f/0x02b1,0x0e6f/0x02b3,0x0e6f/0x02b5,0x0e6f/0x02b6,0x0e6f/0x02bd,0x0e6f/0x02be,0x0e6f/0x02bf,0x0e6f/0x02c0,0x0e6f/0x02c1,0x0e6f/0x02c2,0x0e6f/0x02c3,0x0e6f/0x02c4,0x0e6f/0x02c5,0x0e6f/0x02c6,0x0e6f/0x02c7,0x0e6f/0x02c8,0x0e6f/0x02c9,0x0e6f/0x02ca,0x0e6f/0x02cb,0x0e6f/0x02cd,0x0e6f/0x02ce,0x0e6f/0x02cf,0x0e6f/0x02d5,0x0e6f/0x0346,0x0e6f/0x0446,0x0e6f/0x02da,0x0e6f/0x02d6,0x0e6f/0x02d9,0x0f0d/0x0063,0x0f0d/0x0067,0x0f0d/0x0078,0x0f0d/0x00c5,0x0f0d/0x0150,0x10f5/0x7009,0x10f5/0x7013,0x1532/0x0a00,0x1532/0x0a03,0x1532/0x0a14,0x1532/0x0a15,0x20d6/0x2001,0x20d6/0x2002,0x20d6/0x2003,0x20d6/0x2004,0x20d6/0x2005,0x20d6/0x2006,0x20d6/0x2009,0x20d6/0x200a,0x20d6/0x200b,0x20d6/0x200c,0x20d6/0x200d,0x20d6/0x200e,0x20d6/0x200f,0x20d6/0x2011,0x20d6/0x2012,0x20d6/0x2015,0x20d6/0x2016,0x20d6/0x2017,0x20d6/0x2018,0x20d6/0x2019,0x20d6/0x201a,0x20d6/0x4001,0x20d6/0x4002,0x20d6/0x890b,0x24c6/0x541a,0x24c6/0x542a,0x24c6/0x543a,0x24c6/0x551a,0x24c6/0x561a,0x24c6/0x581a,0x24c6/0x591a,0x24c6/0x592a,0x24c6/0x791a,0x2dc8/0x2002,0x2dc8/0x3106,0x2e24/0x0652,0x2e24/0x1618,0x2e24/0x1688,0x146b/0x0611,0x0000/0x0000,0x045e/0x02a2,0x0e6f/0x1414,0x0e6f/0x0159,0x24c6/0xfaff,0x0f0d/0x006d,0x0f0d/0x00a4,0x0079/0x1832,0x0079/0x187f,0x0079/0x1883,0x03eb/0xff01,0x0c12/0x0ef8,0x046d/0x1000,0x11ff/0x0511,0x1345/0x6006,0x056e/0x2012,0x146b/0x0602,0x0f0d/0x00ae,0x046d/0x0401,0x046d/0x0301,0x046d/0xcaa3,0x046d/0xc261,0x046d/0x0291,0x0079/0x18d3,0x0f0d/0x00b1,0x0001/0x0001,0x0079/0x188e,0x0079/0x187c,0x0079/0x189c,0x0079/0x1874,0x2f24/0x0050,0x2f24/0x002e,0x2f24/0x0091,0x1430/0x0719,0x0f0d/0x00ed,0x0f0d/0x00c0,0x0e6f/0x0152,0x046d/0x1007,0x0e6f/0x02b8,0x0079/0x18a1,0x0000/0x6686,0x12ab/0x0304,0x1430/0x0291,0x1430/0x02a9,0x1430/0x070b,0x1bad/0x028e,0x1bad/0x02a0,0x1bad/0x5500,0x20ab/0x55ef,0x24c6/0x5509,0x2516/0x0069,0x25b1/0x0360,0x2c22/0x2203,0x2f24/0x0011,0x2f24/0x0053,0x2f24/0x00b7,0x046d/0x0000,0x046d/0x1004,0x046d/0x1008,0x046d/0xf301,0x0738/0x02a0,0x0738/0x7263,0x0738/0xb738,0x0738/0xcb29,0x0738/0xf401,0x0079/0x18c2,0x0079/0x18c8,0x0079/0x18cf,0x0c12/0x0e17,0x0c12/0x0e1c,0x0c12/0x0e22,0x0c12/0x0e30,0xd2d2/0xd2d2,0x0d62/0x9a1a,0x0d62/0x9a1b,0x0e00/0x0e00,0x0e6f/0x012a,0x0e6f/0x02b2,0x0f0d/0x0097,0x0f0d/0x00ba,0x0f0d/0x00d8,0x0fff/0x02a1,0x045e/0x0867,0x16d0/0x0f3f,0x2f24/0x008f,0x0e6f/0xf501,0x057e/0x2006,0x057e/0x2067,0x057e/0x2007,0x057e/0x2066,0x057e/0x2008,0x057e/0x2068,0x057e/0x2009,0x057e/0x2069,0x0f0d/0x00c1,0x0f0d/0x0092,0x0f0d/0x00f6,0x0e6f/0x0180,0x0e6f/0x0181,0x0e6f/0x0184,0x0e6f/0x0185,0x0e6f/0x0186,0x0e6f/0x0187,0x0e6f/0x0188,0x0e6f/0x018c,0x0f0d/0x00aa,0x20d6/0xa711,0x20d6/0xa712,0x20d6/0xa713,0x20d6/0xa714,0x20d6/0xa715,0x20d6/0xa716,0x20d6/0xa718,0x33dd/0x0001,0x33dd/0x0002,0x33dd/0x0003,0x0f0d/0x00f0,0x0000/0x11fb,0x28de/0x1101,0x28de/0x1102,0x28de/0x1105,0x28de/0x1106,0x28de/0x1142,0x28de/0x1201,0x28de/0x1202,0x28de/0x1205,0x28de/0x1302,0x28de/0x1303,0x28de/0x1304,0x2dc8/0x9000,0x2dc8/0x3810,0x2dc8/0x0651,0x2dc8/0x9020,0x2dc8/0x9015,0x2dc8/0x2865,0x1235/0xab12,0x2002/0x9000,0x2dc8/0x9001,0x3820/0x0009,0x2dc8/0x3820,0x2dc8/0x2000,0x2dc8/0x2000,0x2810/0x0009,0x2dc8/0x2830,0x2dc8/0x6002,0x2dc8/0x6102,0x1235/0xab20,0x2820/0x0009,0x2dc8/0x301b,0x2dc8/0x3011,0x2dc8/0x3013,0x2dc8/0x9018,0x2dc8/0x3230,0x05a0/0x3232,0x05a0/0x3232,0x2dc8/0x3100,0x2dc8/0x9012,0x2dc8/0x2862,0x0b05/0x4500,0x0b05/0x4500,0x0b05/0x7905,0x0b05/0x7906,0x0010/0x0082,0x1949/0x0402,0x1949/0x0419,0x0171/0x0419,0x0079/0x1830,0x3250/0x1001,0x3250/0x1001,0x3250/0x1002,0x3250/0x1002,0x24c6/0x891b,0x0c12/0x0ef7,0x04b4/0x010a,0xffff/0xffff,0x20e8/0x5860,0x0926/0x8888,0x0e6f/0x0130,0x0079/0x0011,0x1a34/0xf705,0x1949/0x0402,0x3537/0x1097,0x05ac/0x061a,0x25f0/0x83c1,0x18d1/0x9400,0x18d1/0x9400,0x0428/0x4001,0x0e8f/0x1006,0x0e8f/0x0012,0x0f0d/0x0010,0x0f0d/0x0022,0x0f0d/0x006b,0xdead/0xbeef,0x14d8/0x6208,0x0e8f/0x3013,0x04d8/0x0082,0x05fd/0x3000,0x1949/0x0402,0x056e/0x2003,0x0f30/0x0110,0x22ba/0x1020,0x046d/0xc219,0x046d/0xc216,0x046d/0xc216,0x046d/0xc219,0x046d/0xc218,0x046d/0xc211,0x24c6/0x892b,0x24c6/0x892a,0x24c6/0x891a,0x0738/0x5266,0x0738/0x3384,0x0738/0x3480,0x0738/0x8818,0x0078/0x0006,0x045e/0x000e,0x045e/0x0285,0x045e/0x0289,0x045e/0x0289,0x20d6/0x0dad,0x146b/0x0c01,0x0810/0xe501,0x0955/0x7214,0x0955/0x7214,0x124b/0x4d01,0x1345/0x3008,0x0079/0x1843,0x0079/0x1844,0x057e/0x2019,0x057e/0x2019,0x057e/0x201e,0x057e/0x2017,0x057e/0x2017,0x057e/0x2017,0x057e/0x0306,0x057e/0x0330,0x057e/0x0306,0x050d/0x0803,0x2836/0x0001,0x2836/0x0001,0x045e/0x0202,0x11ff/0x3341,0x0e8f/0x0003,0x054c/0x0cda,0x0f30/0x1112,0x2c22/0x2012,0x2c22/0x2010,0x1532/0x0402,0x1532/0x0705,0x1532/0x0900,0x1532/0x0900,0xf000/0x0003,0x0079/0x0011,0x1a34/0x0809,0x7545/0x1122,0x06a3/0xf623,0x06a3/0xff0c,0x06a3/0x040c,0x06a3/0x0109,0x06a3/0x040b,0x06a3/0xf518,0x16c0/0x0487,0x28de/0x11fc,0x0111/0x1431,0x0111/0x1419,0x6666/0x8804,0xf000/0x00f1,0x044f/0xb320,0x044f/0xb323,0x044f/0xb300,0x044f/0xd009,0x044f/0xd008,0x12bd/0xd015,0x14d8/0xcd07,0x0079/0x0011,0x05ac/0x3232,0x0c45/0x4320,0x2717/0x3144,0x16c0/0x05e1,0x6666/0x0667,0x0583/0x2060,0x07b5/0x0315,0x289b/0x0080,0x289b/0x0003,0x289b/0x0060,";

#[cfg(test)]
mod tests {
    use super::{apply_epic_emu_identities_to, build_eos_config, eos_account_id, eos_product_id};
    use crate::handler::{EosConfig, Handler};
    use crate::instance::Instance;
    use crate::util::find_files_named;

    fn instance(profname: &str) -> Instance {
        Instance {
            devices: vec![],
            profname: profname.to_string(),
            profselection: 0,
            monitor: 0,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn eos_id_is_valid_32_char_hex() {
        let id = eos_account_id(".Yokatta", 0);
        assert_eq!(id.len(), 32, "EOS account id must be 32 hex chars");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "EOS account id must be all hex: {id}"
        );
    }

    #[test]
    fn eos_id_is_deterministic_but_distinct_per_instance() {
        // Same (profile, slot) -> stable across calls (and across launches).
        assert_eq!(eos_account_id("Anish", 0), eos_account_id("Anish", 0));
        // Different profiles -> different ids.
        assert_ne!(eos_account_id("Anish", 0), eos_account_id("Bob", 0));
        // Even the degenerate "same profile in two slots" case stays distinct,
        // which is what the lobby actually requires.
        assert_ne!(eos_account_id("Anish", 0), eos_account_id("Anish", 1));
    }

    #[test]
    fn eos_epic_and_product_ids_differ() {
        // EpicId and ProductUserId must be distinct (the emulator keys identity
        // off both), each a valid 32-hex string.
        let e = eos_account_id("Anish", 0);
        let p = eos_product_id("Anish", 0);
        assert_ne!(e, p);
        assert_eq!(p.len(), 32);
        assert!(p.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn build_eos_config_writes_nested_identity_and_options() {
        let eos = EosConfig {
            broadcast_localhost_only: true,
            log_level: "trace".to_string(),
            ..EosConfig::default()
        };
        let v = build_eos_config(&eos, "Yokatta", "00000000000000000000000000000001", "abcdef");
        // Identity lands in the nested schema the emulator actually reads.
        assert_eq!(v["EOSEmu"]["User"]["UserName"], "Yokatta");
        assert_eq!(v["EOSEmu"]["User"]["EpicId"], "00000000000000000000000000000001");
        assert_eq!(v["EOSEmu"]["User"]["ProductUserId"], "abcdef");
        // Options map through to the right places.
        assert_eq!(v["Network"]["Plugins"]["Broadcast"]["Enabled"], true);
        assert_eq!(v["Network"]["Plugins"]["Broadcast"]["LocalhostOnly"], true);
        assert_eq!(v["EOSEmu"]["Application"]["LogLevel"], "trace");
        assert_eq!(v["EOSEmu"]["Ecom"]["UnlockDlcs"], true);
        // trace turns the broadcast log on, otherwise it's quiet.
        assert_eq!(v["Network"]["Plugins"]["Broadcast"]["EnableLog"], true);
    }

    #[test]
    fn build_eos_config_quiet_by_default() {
        let v = build_eos_config(&EosConfig::default(), "Bob", "deadbeef", "f00d");
        assert_eq!(v["EOSEmu"]["Application"]["LogLevel"], "off");
        assert_eq!(v["Network"]["Plugins"]["Broadcast"]["EnableLog"], false);
    }

    #[test]
    fn find_files_named_returns_relative_paths() {
        // Build a tiny tree under a unique temp dir and confirm the walker finds
        // the target at the right relative paths.
        let root = std::env::temp_dir().join("partydeck_find_test_42");
        let _ = std::fs::remove_dir_all(&root);
        let deep = root.join("BET/Binaries/Win64/nepice_settings");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("NemirtingasEpicEmu.json"), "{}").unwrap();
        std::fs::write(
            root.join("BET/Binaries/Win64/NemirtingasEpicEmu.json"),
            "{}",
        )
        .unwrap();
        std::fs::write(root.join("unrelated.txt"), "x").unwrap();

        let mut found = find_files_named(&root, "NemirtingasEpicEmu.json");
        found.sort();
        assert_eq!(found.len(), 2, "should find both configs, got {found:?}");
        assert!(found.contains(&std::path::PathBuf::from(
            "BET/Binaries/Win64/NemirtingasEpicEmu.json"
        )));
        assert!(found.contains(&std::path::PathBuf::from(
            "BET/Binaries/Win64/nepice_settings/NemirtingasEpicEmu.json"
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_identities_writes_distinct_per_instance_files() {
        // Synthetic handler whose overlay ships one static EOS config.
        let base = std::env::temp_dir().join("partydeck_apply_test_7");
        let _ = std::fs::remove_dir_all(&base);
        let cfg_dir = base
            .join("handler/overlay/BET/Binaries/Win64/nepice_settings");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("NemirtingasEpicEmu.json"),
            r#"{"enable_lan": true, "savepath": "appdata", "username": "DefaultName"}"#,
        )
        .unwrap();

        let mut h = Handler::default();
        h.path_handler = base.join("handler");
        h.eos.enabled = true;

        let instances = vec![instance(".Yokatta"), instance("Anish")];
        let tmp = base.join("tmp");
        apply_epic_emu_identities_to(&h, &instances, &tmp).unwrap();

        let rel = "BET/Binaries/Win64/nepice_settings/NemirtingasEpicEmu.json";
        let v0: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join("game-0").join(rel)).unwrap(),
        )
        .unwrap();
        let v1: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join("game-1").join(rel)).unwrap(),
        )
        .unwrap();

        // Names assigned from profiles (guest dot stripped), options generated.
        assert_eq!(v0["EOSEmu"]["User"]["UserName"], "Yokatta");
        assert_eq!(v1["EOSEmu"]["User"]["UserName"], "Anish");
        assert_eq!(v0["Network"]["Plugins"]["Broadcast"]["Enabled"], true);
        assert_eq!(v1["EOSEmu"]["Application"]["SavePath"], "appdata");
        // The whole point: the two instances get DIFFERENT EOS ids.
        assert_ne!(
            v0["EOSEmu"]["User"]["EpicId"],
            v1["EOSEmu"]["User"]["EpicId"]
        );
        assert_eq!(v0["EOSEmu"]["User"]["EpicId"].as_str().unwrap().len(), 32);

        let _ = std::fs::remove_dir_all(&base);
    }
}
