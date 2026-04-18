mod app;
mod handler;
mod input;
mod instance;
mod launch;
mod monitor;
mod paths;
mod profiles;
mod util;

use crate::app::*;
use crate::handler::Handler;
use crate::monitor::{get_monitors_errorless, get_x11_dpi_scale};
use crate::paths::PATH_PARTY;
use crate::profiles::remove_guest_profiles;
use crate::util::*;

fn main() -> eframe::Result {
    if std::env::args().any(|arg| arg == "--help") {
        println!("{}", USAGE_TEXT);
        std::process::exit(0);
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

    let args: Vec<String> = std::env::args().collect();

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

    remove_guest_profiles().unwrap();
    clear_tmp().unwrap();

    let scrheight = monitors[0].height();

    let scale = match fullscreen {
        true => scrheight as f32 / 560.0 / get_x11_dpi_scale(),
        false => 1.3,
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 540.0])
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
    --exec <executable>   Execute the specified executable in splitscreen. If this isn't specified, PartyDeck will launch in the regular GUI mode.
    --args [args]         Specify arguments for the executable to be launched with. Must be quoted if containing spaces.
    --fullscreen          Start the GUI in fullscreen mode
    --kwin                Launch PartyDeck inside of a KWin session
"#;
