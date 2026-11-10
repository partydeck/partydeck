mod app;
mod handler;
mod input;
mod instance;
mod launch;
mod monitor;
mod paths;
mod profiles;
mod util;

use std::error::Error;
use std::path::PathBuf;

use crate::app::*;
use crate::handler::Handler;
use crate::monitor::{get_monitors_errorless, get_x11_dpi_scale};
use crate::paths::PATH_PARTY;
use crate::profiles::remove_guest_profiles;
use crate::util::*;

use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Automatically build a simple handler for an executable. If this isn't specified, PartyDeck will launch in the regular GUI mode.
    #[arg(long, value_name = "executable")]
    exec: Option<String>,

    /// Specify arguments for the executable to be launched with. Must be quoted if containing spaces.
    #[arg(long, value_name = "args", default_value = "", allow_hyphen_values = true)]
    args: String,

    /// Open to a specific handler. Takes either an installed handler's name or a handler.json path
    #[arg(long, value_name = "handler", conflicts_with = "exec")]
    handler: Option<String>,

    /// Start the GUI in fullscreen mode
    #[arg(long)]
    fullscreen: bool,

    /// Launch PartyDeck inside of a KWin session
    #[arg(long)]
    kwin: bool,
}

/// Resolve a handler from CLI input: either a handler.json path, a handler directory, 
/// or an installed handler's directory name or display name.
fn resolve_handler(spec: &str) -> Result<Handler, Box<dyn Error>> {
    let path = PathBuf::from(spec);
    // From json file
    if path.is_file() {
        return Handler::from_json(&path);
    }
    // From handler dir
    if path.join("handler.json").is_file() {
        return Handler::from_json(&path.join("handler.json"));
    }
    // From installed handler dir name
    let mut handlers = handler::scan_handlers();
    if let Some(h) = handlers.iter().find(|h| h.handler_dir_name() == spec) {
        return Ok(h.clone());
    }
    // From installed handler label
    handlers.retain(|h| h.name.eq_ignore_ascii_case(spec));
    match handlers.len() {
        0 => Err(format!("No handler file or installed handler named \"{spec}\"").into()),
        1 => Ok(handlers.remove(0)),
        _ => {
            let dirs: Vec<&str> = handlers.iter().map(|h| h.handler_dir_name()).collect();
            Err(format!(
                "Multiple handlers named '{spec}'; pass one of these instead: {}",
                dirs.join(", ")
            ).into())
        }
    }
}

fn main() -> eframe::Result {
    let cli = Cli::parse();

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

    if cli.kwin {
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

    let handler_exclusive = if let Some(exec) = &cli.exec {
        Some(Handler::from_cli(exec, &cli.args))
    } else if let Some(spec) = &cli.handler {
        let handler = resolve_handler(spec).unwrap_or_else(|err| {
            eprintln!("[partydeck] {err}");
            std::process::exit(1);
        });
        Some(handler)
    } else {
        None
    };

    let fullscreen = cli.fullscreen;

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
                handler_exclusive,
            )))
        }),
    )
}
