use crate::app::PartyConfig;
use crate::monitor::Monitor;
use crate::profiles::GUEST_NAMES;

#[derive(Clone)]
pub struct Instance {
    pub devices: Vec<usize>,
    pub profname: String,
    pub profselection: usize,
    pub monitor: usize,
    pub width: u32,
    pub height: u32,
}

pub fn set_instance_resolutions(
    instances: &mut Vec<Instance>,
    primary_monitor: &Monitor,
    cfg: &PartyConfig,
    use_river_layout: bool,
) {
    let (basewidth, baseheight) = (primary_monitor.width(), primary_monitor.height());
    let playercount = instances.len() as u32;
    if use_river_layout {
        let (mut w, mut h) = (0 as u32, 0 as u32);
        while w * h < playercount {
            if basewidth * h * 9 > baseheight * w * 16 {
                w += 1;
            } else {
                h += 1;
            }
        }

        while playercount <= w * (h - 1) {
            h -= 1
        }
        while playercount <= (w - 1) * h {
            w -= 1
        }

        for i in 0..playercount {
            let mut cur_row_width = w - ((i % h >= (playercount - 1) % h + 1) as u32);

            if cur_row_width == 0 {
                cur_row_width = 1;
            }

            let instance = &mut instances[i as usize];

            instance.width = (basewidth / cur_row_width) as u32;
            instance.height = (baseheight / h) as u32;
        }
    } else {
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
            instance.width = w;
            instance.height = h;
        }
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
