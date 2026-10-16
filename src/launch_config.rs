use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::app::{load_cfg, PartyConfig};
use crate::handler::{scan_handlers, Handler};
use crate::input::{scan_input_devices, DeviceInfo, DeviceType, InputDevice};
use crate::instance::{
    set_instance_names, set_instance_resolutions, set_instance_resolutions_multimonitor, Instance,
};
use crate::launch::{fuse_overlayfs_mount_gamedirs, launch_game, setup_profiles};
use crate::monitor::Monitor;
use crate::paths::PATH_PARTY;
use crate::profiles::{remove_guest_profiles, scan_profiles, GUEST_NAMES};
use crate::util::{clear_tmp, kwin_dbus_unload_script};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchConfig {
    #[serde(default)]
    schema_version: Option<u32>,
    handler: String,
    instances: Vec<LaunchInstance>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchInstance {
    controllers: ControllerSelection,
    profile: String,
    #[serde(default)]
    monitor: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ControllerSelection {
    One(String),
    Many(Vec<String>),
}

impl ControllerSelection {
    fn names(&self) -> Vec<&str> {
        match self {
            Self::One(name) => vec![name.as_str()],
            Self::Many(names) => names.iter().map(String::as_str).collect(),
        }
    }
}

struct ResolvedLaunch {
    handler: Handler,
    cfg: PartyConfig,
    instances: Vec<Instance>,
    device_infos: Vec<DeviceInfo>,
    device_labels: Vec<(String, String)>,
}

pub fn run_launch_config(
    config_path: &str,
    monitors: Vec<Monitor>,
    dry_run: bool,
) -> Result<(), Box<dyn Error>> {
    let config_contents = fs::read_to_string(config_path)
        .map_err(|e| format!("Could not read launch config '{}': {e}", config_path))?;
    let config: LaunchConfig = serde_json::from_str(&config_contents)
        .map_err(|e| format!("Invalid launch JSON in '{}': {e}", config_path))?;

    if let Some(version) = config.schema_version {
        if version != 1 {
            return Err(format!("Unsupported launch config schema_version {version}; this build supports version 1.").into());
        }
    }
    if config.handler.trim().is_empty() {
        return Err("The launch config field 'handler' cannot be empty.".into());
    }
    if config.instances.is_empty() {
        return Err("The launch config must contain at least one instance.".into());
    }
    let guest_count = config.instances.iter().filter(|instance| instance.profile == "Guest").count();
    if guest_count > GUEST_NAMES.len() {
        return Err(format!(
            "This PartyDeck build supports at most {} Guest instances in one launch.",
            GUEST_NAMES.len()
        ).into());
    }
    if monitors.is_empty() {
        return Err("No monitors were found, so PartyDeck cannot resolve instance monitor assignments.".into());
    }

    // Mirror the GUI's startup initialization without creating an eframe window.
    fs::create_dir_all(PATH_PARTY.join("handlers"))?;
    fs::create_dir_all(PATH_PARTY.join("profiles"))?;
    if !PATH_PARTY.join("goldberg_data").exists() {
        let settings_path = PATH_PARTY.join("goldberg_data/steam_settings");
        fs::create_dir_all(&settings_path)?;
        fs::write(settings_path.join("auto_accept_invite.txt"), "")?;
        fs::write(settings_path.join("auto_send_invite.txt"), "")?;
    }

    let resolved = resolve_launch(config, &monitors)?;
    print_launch_plan(&resolved, &monitors);

    if dry_run {
        println!("[partydeck] Dry run complete; the game was not launched.");
        return Ok(());
    }

    // As in normal PartyDeck startup, remove stale Guest profiles and temp files only
    // when actually launching. A validation-only run should not alter session data.
    remove_guest_profiles()?;
    clear_tmp()?;

    let launch_result: Result<(), Box<dyn Error>> = (|| {
        setup_profiles(&resolved.handler, &resolved.instances)?;
        if resolved.handler.is_saved_handler()
            && !resolved.cfg.disable_mount_gamedirs
            && resolved.cfg.profile_unique_dirs
        {
            fuse_overlayfs_mount_gamedirs(&resolved.handler, &resolved.instances)?;
        }
        launch_game(
            &resolved.handler,
            &resolved.device_infos,
            &resolved.instances,
            &resolved.cfg,
        )?;
        Ok(())
    })();

    if resolved.cfg.enable_kwin_script {
        if let Err(err) = kwin_dbus_unload_script() {
            eprintln!("[partydeck] Warning: failed to unload the KWin script after launch: {err}");
        }
    }
    if let Err(err) = remove_guest_profiles() {
        eprintln!("[partydeck] Warning: failed to remove temporary Guest profiles: {err}");
    }
    if let Err(err) = clear_tmp() {
        eprintln!("[partydeck] Warning: failed to clear temporary launch files: {err}");
    }

    launch_result?;
    println!("[partydeck] Configured game session finished.");
    Ok(())
}

fn resolve_launch(
    config: LaunchConfig,
    monitors: &[Monitor],
) -> Result<ResolvedLaunch, Box<dyn Error>> {
    let handler = resolve_handler(&config.handler)?;
    let base_cfg = load_cfg();
    let mut cfg = base_cfg.with_overrides(&handler.overrides);
    let running_in_plasma = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|desktop| desktop == "KDE")
        .unwrap_or(false)
        || std::env::var("HOMETEST_DESKTOP_SESSION")
            .map(|desktop| desktop == "plasma")
            .unwrap_or(false);
    if !running_in_plasma {
        cfg.enable_kwin_script = false;
    }
    let input_devices = scan_input_devices(&cfg.pad_filter_type);
    let profiles = scan_profiles(true);

    let mut owner_by_device: HashMap<usize, usize> = HashMap::new();
    let mut instances = Vec::with_capacity(config.instances.len());

    for (instance_index, selection) in config.instances.iter().enumerate() {
        if selection.profile.trim().is_empty() {
            return Err(format!("Instance {} has an empty profile name.", instance_index + 1).into());
        }

        let profile_selection = profiles
            .iter()
            .position(|profile| profile == &selection.profile)
            .ok_or_else(|| {
                format!(
                    "Unknown profile '{}' for instance {}. Available profiles: {}",
                    selection.profile,
                    instance_index + 1,
                    profiles.join(", ")
                )
            })?;

        let requested_monitor = selection
            .monitor
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| monitors[0].name());
        let monitor_index = monitors
            .iter()
            .position(|monitor| monitor.name() == requested_monitor)
            .ok_or_else(|| {
                format!(
                    "Unknown monitor '{}' for instance {}. Available monitors: {}",
                    requested_monitor,
                    instance_index + 1,
                    monitors.iter().map(|monitor| monitor.name()).collect::<Vec<_>>().join(", ")
                )
            })?;

        if !cfg.gamescope_sdl_backend && monitor_index != 0 {
            return Err(format!(
                "Instance {} selects monitor '{}', but the Gamescope SDL backend is disabled. Enable it in PartyDeck settings to use non-primary monitors.",
                instance_index + 1,
                requested_monitor
            ).into());
        }

        let controller_names = selection.controllers.names();
        if controller_names.is_empty() {
            return Err(format!("Instance {} must specify at least one controller.", instance_index + 1).into());
        }

        let mut devices = Vec::with_capacity(controller_names.len());
        for controller_name in controller_names {
            if controller_name.trim().is_empty() {
                return Err(format!("Instance {} contains an empty controller name.", instance_index + 1).into());
            }
            let device_index = resolve_controller(controller_name, &input_devices, instance_index + 1)?;
            if devices.contains(&device_index) {
                return Err(format!(
                    "Instance {} lists controller '{}' more than once.",
                    instance_index + 1,
                    controller_name
                ).into());
            }
            if !cfg.allow_multiple_instances_on_same_device
                && let Some(previous_instance) = owner_by_device.insert(device_index, instance_index)
            {
                return Err(format!(
                    "Controller '{}' is assigned to both instances {} and {}. PartyDeck currently disallows sharing a controller between instances unless 'allow_multiple_instances_on_same_device' is enabled.",
                    controller_name,
                    previous_instance + 1,
                    instance_index + 1
                ).into());
            }
            devices.push(device_index);
        }

        instances.push(Instance {
            devices,
            profname: String::new(),
            profselection: profile_selection,
            monitor: monitor_index,
            width: 0,
            height: 0,
        });
    }

    if cfg.gamescope_sdl_backend {
        set_instance_resolutions_multimonitor(&mut instances, &monitors.to_vec(), &cfg);
    } else {
        set_instance_resolutions(&mut instances, &monitors[0], &cfg);
    }
    set_instance_names(&mut instances, &profiles);

    let device_infos = input_devices.iter().map(InputDevice::info).collect();
    let device_labels = input_devices
        .iter()
        .map(|device| (device.name().to_string(), device.path().to_string()))
        .collect();

    Ok(ResolvedLaunch {
        handler,
        cfg,
        instances,
        device_infos,
        device_labels,
    })
}

fn resolve_controller(
    selector: &str,
    input_devices: &[InputDevice],
    instance_number: usize,
) -> Result<usize, Box<dyn Error>> {
    let matches: Vec<usize> = input_devices
        .iter()
        .enumerate()
        .filter(|(_, device)| {
            device.enabled()
                && device.device_type() == DeviceType::Gamepad
                && (device.name() == selector
                    || device.fancyname() == selector
                    || device.selector_name() == selector
                    || device.path() == selector)
        })
        .map(|(index, _)| index)
        .collect();

    match matches.as_slice() {
        [index] => Ok(*index),
        [] => {
            let available = input_devices
                .iter()
                .filter(|device| device.device_type() == DeviceType::Gamepad)
                .map(|device| {
                    format!(
                        "{} / raw name '{}' [{}]{}",
                        device.selector_name(),
                        device.name(),
                        device.path(),
                        if device.enabled() { "" } else { " (disabled by PartyDeck input filter)" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "Controller '{}' for instance {} was not found among enabled gamepads. Available gamepads: {}. Run 'partydeck --list-inputs' to inspect names and paths.",
                selector,
                instance_number,
                if available.is_empty() { "(none)".to_string() } else { available }
            ).into())
        }
        _ => {
            let paths = matches
                .iter()
                .map(|index| input_devices[*index].path().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "Controller selector '{}' is ambiguous for instance {} (matching device paths: {}). Use the exact 'selector' or 'path' from 'partydeck --list-inputs'.",
                selector, instance_number, paths
            ).into())
        }
    }
}

fn handler_from_candidate(path: &Path) -> Result<Option<Handler>, Box<dyn Error>> {
    if path.is_dir() {
        let handler_json = path.join("handler.json");
        if handler_json.is_file() {
            return Ok(Some(Handler::from_json(&handler_json)?));
        }
    } else if path.is_file() {
        return Ok(Some(Handler::from_json(&path.to_path_buf())?));
    }
    Ok(None)
}

fn resolve_handler(selector: &str) -> Result<Handler, Box<dyn Error>> {
    let raw_path = PathBuf::from(selector);
    if let Some(handler) = handler_from_candidate(&raw_path)? {
        return Ok(handler);
    }

    let installed_candidate = PATH_PARTY.join("handlers").join(selector);
    if let Some(handler) = handler_from_candidate(&installed_candidate)? {
        return Ok(handler);
    }

    let selector_path = Path::new(selector);
    let selector_stem = selector_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(selector);

    let handlers = scan_handlers();
    for handler in &handlers {
        if handler.name.eq_ignore_ascii_case(selector)
            || handler.name.eq_ignore_ascii_case(selector_stem)
            || handler.handler_dir_name().eq_ignore_ascii_case(selector)
            || handler.handler_dir_name().eq_ignore_ascii_case(selector_stem)
        {
            return Ok(handler.clone());
        }
    }

    // Also allow a JSON filename within an installed handler's directory.
    if let Some(filename) = selector_path.file_name().and_then(|name| name.to_str()) {
        if let Ok(entries) = fs::read_dir(PATH_PARTY.join("handlers")) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let candidate = entry.path().join(filename);
                    if candidate.is_file() {
                        return Ok(Handler::from_json(&candidate)?);
                    }
                }
            }
        }
    }

    let available = handlers
        .iter()
        .map(|handler| format!("{} ({})", handler.name, handler.handler_dir_name()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "Handler '{}' was not found. Use an installed handler name/folder or a path to a handler JSON file. Available handlers: {}",
        selector,
        if available.is_empty() { "(none)".to_string() } else { available }
    ).into())
}

fn print_launch_plan(resolved: &ResolvedLaunch, monitors: &[Monitor]) {
    println!("[partydeck] Handler: {} ({})", resolved.handler.name, resolved.handler.path_handler.display());
    for (index, instance) in resolved.instances.iter().enumerate() {
        let controllers = instance
            .devices
            .iter()
            .map(|device_index| {
                let (name, path) = &resolved.device_labels[*device_index];
                format!("{} [{}]", name, path)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let monitor_name = monitors
            .get(instance.monitor)
            .map(Monitor::name)
            .unwrap_or("unknown");
        let profile = if instance.profname.starts_with('.') {
            format!("Guest ({})", instance.profname.trim_start_matches('.'))
        } else {
            instance.profname.clone()
        };
        println!(
            "[partydeck] Instance {}: profile={}, controllers={}, monitor={}, resolution={}x{}",
            index + 1,
            profile,
            controllers,
            monitor_name,
            instance.width,
            instance.height
        );
    }
}
