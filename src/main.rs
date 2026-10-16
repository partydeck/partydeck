mod app;
mod handler;
mod input;
mod instance;
mod launch;
mod monitor;
mod paths;
mod profiles;
mod util;
mod launch_config;

use crate::app::*;
use crate::handler::Handler;
use crate::input::{scan_input_devices, DeviceType};
use crate::monitor::{get_monitors_errorless, get_x11_dpi_scale};
use crate::paths::PATH_PARTY;
use crate::profiles::{remove_guest_profiles, scan_profiles};
use crate::util::*;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|arg| arg == "--help") {
        println!("{}", USAGE_TEXT);
        std::process::exit(0);
    }

    // Machine-readable inventory commands are handled before normal startup logging
    // so stdout remains valid JSON for callers such as Decky.
    if args.iter().any(|arg| arg == "--list-inputs") {
        let cfg = load_cfg();
        let devices = scan_input_devices(&cfg.pad_filter_type);
        let entries: Vec<serde_json::Value> = devices
            .iter()
            .filter(|device| device.device_type() == DeviceType::Gamepad)
            .map(|device| serde_json::json!({
                "name": device.fancyname(),
                "raw_name": device.name(),
                "selector": device.selector_name(),
                "path": device.path(),
                "enabled": device.enabled()
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).expect("Failed to serialize input devices"));
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--list-monitors") {
        let monitors = get_monitors_errorless();
        let entries: Vec<serde_json::Value> = monitors
            .iter()
            .map(|monitor| serde_json::json!({
                "name": monitor.name(),
                "width": monitor.width(),
                "height": monitor.height()
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).expect("Failed to serialize monitors"));
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--list-profiles") {
        println!("{}", serde_json::to_string_pretty(&scan_profiles(true)).expect("Failed to serialize profiles"));
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--list-handlers") {
        let entries: Vec<serde_json::Value> = crate::handler::scan_handlers()
            .iter()
            .map(|handler| serde_json::json!({
                "name": handler.name,
                "directory": handler.handler_dir_name(),
                "handler_json": handler.path_handler.join("handler.json").display().to_string()
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).expect("Failed to serialize handlers"));
        return Ok(());
    }

    let monitors = get_monitors_errorless();

    println!("[partydeck] Monitors detected:");
    for monitor in &monitors {
        println!(
            "[partydeck] {} ({}x{})",
            monitor.name(),
            monitor.width(),
            monitor.height()
        );
    }

    if std::env::args().any(|arg| arg == "--kwin") {
        let args: Vec<String> = std::env::args().filter(|arg| arg != "--kwin").collect();

        let (w, h) = (monitors[0].width(), monitors[0].height());
        let mut cmd = std::process::Command::new("kwin_wayland");

        cmd.arg("--xwayland");
        cmd.arg("--width");
        cmd.arg(w.to_string());
        cmd.arg("--height");
        cmd.arg(h.to_string());
        cmd.arg("--exit-with-session");
        cmd.env("PARTYDECK_SCREEN_WIDTH", w.to_string());
        cmd.env("PARTYDECK_SCREEN_HEIGHT", h.to_string());
        let args_string = args
            .iter()
            .map(|arg| format!("\"{}\"", arg))
            .collect::<Vec<String>>()
            .join(" ");
        cmd.arg(args_string);

        println!("[partydeck] Launching kwin session: {:?}", cmd);

        match cmd.spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                eprintln!("[partydeck] Failed to start kwin_wayland: {}", e);
                std::process::exit(1);
            }
        }
    }

    let launch_config_path = if let Some(index) = args.iter().position(|arg| arg == "--launch-config") {
        match args.get(index + 1) {
            Some(path) if !path.starts_with("--") => Some(path.clone()),
            _ => {
                eprintln!("[partydeck] --launch-config requires a JSON file path.");
                std::process::exit(2);
            }
        }
    } else {
        None
    };

    if args.iter().any(|arg| arg == "--dry-run") && launch_config_path.is_none() {
        eprintln!("[partydeck] --dry-run requires --launch-config <json>.");
        std::process::exit(2);
    }
    if launch_config_path.is_some() && args.iter().any(|arg| arg == "--exec") {
        eprintln!("[partydeck] --launch-config and --exec cannot be used together.");
        std::process::exit(2);
    }

    let mut exec = String::new();
    let mut execargs = String::new();
    if let Some(exec_index) = args.iter().position(|arg| arg == "--exec") {
        if let Some(next_arg) = args.get(exec_index + 1) {
            exec = next_arg.clone();
        } else {
            eprintln!("{}", USAGE_TEXT);
            std::process::exit(1);
        }
    }
    if let Some(execargs_index) = args.iter().position(|arg| arg == "--args") {
        if let Some(next_arg) = args.get(execargs_index + 1) {
            execargs = next_arg.clone();
        } else {
            eprintln!("{}", USAGE_TEXT);
            std::process::exit(1);
        }
    }

    let handler_lite = if !exec.is_empty() {
        Some(Handler::from_cli(&exec, &execargs))
    } else {
        None
    };

    let fullscreen = std::env::args().any(|arg| arg == "--fullscreen");

    std::fs::create_dir_all(PATH_PARTY.join("handlers"))
        .expect("Failed to create handlers directory");
    std::fs::create_dir_all(PATH_PARTY.join("profiles"))
        .expect("Failed to create profiles directory");
    if !PATH_PARTY.join("goldberg_data").exists() {
        std::fs::create_dir_all(PATH_PARTY.join("goldberg_data/steam_settings"))
            .expect("Failed to create goldberg data!");
        std::fs::write(PATH_PARTY.join("goldberg_data/steam_settings/auto_accept_invite.txt"), "").expect("failed to create auto_accept_invite.txt");
        std::fs::write(PATH_PARTY.join("goldberg_data/steam_settings/auto_send_invite.txt"), "").expect("failed to create auto_send_invite.txt");
    }

    if let Some(config_path) = launch_config_path {
        let dry_run = args.iter().any(|arg| arg == "--dry-run");
        if let Err(err) = launch_config::run_launch_config(&config_path, monitors.clone(), dry_run) {
            eprintln!("[partydeck] Launch configuration failed: {err}");
            std::process::exit(1);
        }
        return Ok(());
    }

    remove_guest_profiles().unwrap();
    clear_tmp().unwrap();

    let scrheight = monitors[0].height();

    let scale = match fullscreen {
        true => scrheight as f32 / 560.0 / get_x11_dpi_scale(),
        false => 1.3,
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 580.0])
            .with_min_inner_size([640.0, 360.0])
            .with_fullscreen(fullscreen)
            .with_icon(
                eframe::icon_data::from_png_bytes(&include_bytes!("../res/icon.png")[..])
                    .expect("Failed to load icon"),
            ),
        ..Default::default()
    };

    println!("[partydeck] Starting eframe app...");

    eframe::run_native(
        "PartyDeck",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_zoom_factor(scale);
            Ok(Box::<PartyApp>::new(PartyApp::new(
                monitors.clone(),
                handler_lite,
            )))
        }),
    )
}

static USAGE_TEXT: &str = r#"
Usage: partydeck [OPTIONS]

Options:
    --exec <executable>   Use a temporary executable handler in the interactive GUI.
    --args [args]         Arguments for --exec. Must be quoted if containing spaces.
    --launch-config <json> Launch a configured handler and its player instances without opening the GUI.
    --dry-run             Validate and print a launch plan without starting the game (requires --launch-config).
    --list-inputs         Print gamepad names, device paths, and enabled state as JSON.
    --list-monitors       Print monitor names and resolutions as JSON.
    --list-profiles       Print profile names, including Guest, as JSON.
    --list-handlers       Print installed handlers as JSON.
    --fullscreen          Start the GUI in fullscreen mode
    --kwin                Launch PartyDeck inside of a KWin session
"#;
