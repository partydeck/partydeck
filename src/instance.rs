use crate::monitor::Monitor;
use crate::app::PartyConfig;
use crate::profiles::GUEST_NAMES;

#[derive(Clone)]
pub struct Instance {
    pub devices: Vec<usize>,
    pub profname: String,
    pub profselection: usize,
    pub monitor: usize,
    /// Audio sink name for this instance's `PULSE_SINK` ("" = system default).
    pub audio_sink: String,
    pub width: u32,
    pub height: u32,
    /// Manual per-instance render-resolution override. `None` = render at the
    /// monitor/tile size (default). `Some((w, h))` makes the game render at w×h
    /// (gamescope `-w/-h`) while still being upscaled to fill the assigned
    /// monitor (gamescope `-W/-H`) — e.g. force 1600×900 on a 1920×1200 screen.
    pub res_override: Option<(u32, u32)>,
}

pub fn set_instance_resolutions(
    instances: &mut Vec<Instance>,
    primary_monitor: &Monitor,
    cfg: &PartyConfig,
) {
    let (basewidth, baseheight) = (primary_monitor.width(), primary_monitor.height());
    let playercount = instances.len();

    for instance in instances {
        let (mut w, mut h) = match playercount {
            1 => (basewidth, baseheight),
            2 => {
                if cfg.vertical_two_player {
                    (basewidth / 2, baseheight)
                } else {
                    (basewidth, baseheight / 2)
                }
            }
            _ => (basewidth / 2, baseheight / 2),
        };
        if h < 600 && cfg.gamescope_fix_lowres {
            let ratio = w as f32 / h as f32;
            h = 600;
            w = (h as f32 * ratio) as u32;
        }
        // res_override drives the gamescope *nested* render size (-w/-h) in
        // launch.rs; the output (-W/-H) stays at the monitor size so the window
        // still fills the screen.
        instance.width = w;
        instance.height = h;
    }
}

pub fn set_instance_resolutions_multimonitor(
    instances: &mut Vec<Instance>,
    monitors: &Vec<Monitor>,
    cfg: &PartyConfig,
) {
    let mut mon_playercounts: Vec<usize> = vec![0; monitors.len()];
    for instance in instances.iter() {
        let mon = instance.monitor;
        mon_playercounts[mon] += 1;
    }

    for instance in instances.iter_mut() {
        let playercount = mon_playercounts[instance.monitor];
        let (basewidth, baseheight) = (
            monitors[instance.monitor].width(),
            monitors[instance.monitor].height(),
        );

        let (mut w, mut h) = match playercount {
            1 => (basewidth, baseheight),
            2 => {
                if cfg.vertical_two_player {
                    (basewidth / 2, baseheight)
                } else {
                    (basewidth, baseheight / 2)
                }
            }
            _ => (basewidth / 2, baseheight / 2),
        };
        if h < 600 && cfg.gamescope_fix_lowres {
            let ratio = w as f32 / h as f32;
            h = 600;
            w = (h as f32 * ratio) as u32;
        }
        // res_override drives the gamescope *nested* render size (-w/-h) in
        // launch.rs; the output (-W/-H) stays at the monitor size so the window
        // still fills the screen.
        instance.width = w;
        instance.height = h;
    }
}

pub fn set_instance_names(instances: &mut Vec<Instance>, profiles: &[String]) {
    let mut guests = GUEST_NAMES.to_vec();

    for instance in instances {
        if instance.profselection == 0 {
            let i = fastrand::usize(..guests.len());
            instance.profname = format!(".{}", guests[i]);
            guests.swap_remove(i);
        } else {
            instance.profname = profiles[instance.profselection].to_owned();
        }
    }
}
