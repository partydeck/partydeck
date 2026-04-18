use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;


#[derive(Clone)]
pub struct Monitor {
    name: String,
    width: u32,
    height: u32,
}

impl Monitor {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

// This should mimic the SDL monitor retrival used by gamescope, while avoiding all of SDL. (IGNORES SDL_HINT_VIDEO_DISPLAY_PRIORITY, and if display dosnt have "visual info" because all modern one will)
// https://github.com/libsdl-org/SDL/blob/225fb12ae13b70689bcb8c0b42bf061120fefcc4/src/video/x11/SDL_x11modes.c#L868
fn get_monitors_x11() -> Result<Vec<Monitor>, Box<dyn std::error::Error>> {
    let (con, screen_num) = x11rb::connect(None)?;
    let screen = &con.setup().roots[screen_num];

    // Get primary output (sorted first in sdl, but as sdl comments say, this should be done already.)
    let primary = con
        .randr_get_output_primary(screen.root)?
        .reply()?
        .output;

    let res = con
        .randr_get_screen_resources(screen.root)?
        .reply()?;

    let mut monitors = Vec::new();

    for output in &res.outputs {
        let info = con
            .randr_get_output_info(*output, res.config_timestamp)?
            .reply()?;

        if info.connection != x11rb::protocol::randr::Connection::CONNECTED || info.crtc == 0 {
            continue;
        }

        let crtc = con
            .randr_get_crtc_info(info.crtc, res.config_timestamp)?
            .reply()?;

        let name = String::from_utf8_lossy(&info.name).to_string();

        let monitor = Monitor {
            name: name.clone(),
            width: crtc.width.into(),
            height: crtc.height.into(),
        };

        if *output == primary {
            // Insert primary at the front (SDL requirement for some reason)
            monitors.insert(0, monitor);
        } else {
            monitors.push(monitor);
        }
    }

    Ok(monitors)
}

pub fn get_x11_dpi_scale() -> f32 {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return 1.0;
    };
    let root = conn.setup().roots[screen_num].root;

    let Ok(cookie) = conn.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 65536) else {
        return 1.0;
    };
    let Ok(reply) = cookie.reply() else {
        return 1.0;
    };

    let rm_string = String::from_utf8_lossy(&reply.value);
    for line in rm_string.lines() {
        if let Some(rest) = line.strip_prefix("Xft.dpi:") {
            if let Ok(dpi) = rest.trim().parse::<f32>() {
                if dpi > 0.0 { return dpi / 96.0; }
            }
        }
    }

    1.0
}

pub fn get_monitors_errorless() -> Vec<Monitor> {
    let mut monitors = Vec::new();

    if let Ok(ret_monitors) = get_monitors_x11() {
        monitors = ret_monitors;
    }

    if monitors.len() == 0 { // Quick patch for those who have no x11 visable monitors, so we dont just panic.
        println!("[PARTYDECK] Failed to get monitors; using assumed 1920x1080");
        monitors.push(Monitor {name: "Partydeck Virtual Monitor".to_string(), width: 1920, height: 1080});
    }

    if let (Ok(w), Ok(h)) = (std::env::var("PARTYDECK_SCREEN_WIDTH"), std::env::var("PARTYDECK_SCREEN_HEIGHT")) {
        if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
            monitors[0].width = w;
            monitors[0].height = h;
        }
    }

    monitors
}