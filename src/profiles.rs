use std::error::Error;
use std::path::PathBuf;

use crate::{handler::Handler, paths::*, util::copy_dir_recursive};

/// Path to a profile's optional EOS display-name override.
fn profile_eos_name_path(name: &str) -> PathBuf {
    PATH_PARTY.join(format!("profiles/{name}/eos_username"))
}

/// A profile's pinned EOS display name, if it has overridden the default (which
/// is the profile's own name). `None` -> inherit the profile name at launch.
pub fn read_profile_eos_username(name: &str) -> Option<String> {
    let s = std::fs::read_to_string(profile_eos_name_path(name)).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Pin (or clear, when `username` is blank) a profile's EOS display-name
/// override. Clearing reverts the EOS name to the profile name.
pub fn write_profile_eos_username(name: &str, username: &str) -> Result<(), std::io::Error> {
    let path = profile_eos_name_path(name);
    if username.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, username.trim())
}

/// The effective EOS display name for a profile: its override if set, else the
/// profile name (guest leading-'.' stripped, matching the in-game name).
pub fn profile_eos_display(name: &str) -> String {
    read_profile_eos_username(name).unwrap_or_else(|| name.trim_start_matches('.').to_string())
}

// Makes a folder and sets up Goldberg Steam Emu profile for Steam games
pub fn create_profile(name: &str) -> Result<(), std::io::Error> {
    if PATH_PARTY.join(format!("profiles/{name}")).exists() {
        return Ok(());
    }

    println!("[partydeck] Creating profile {name}");
    let path_profile = PATH_PARTY.join(format!("profiles/{name}"));
    let path_steam = path_profile.join("steam/settings");

    std::fs::create_dir_all(path_profile.join("windata/AppData/Local/Temp"))?;
    std::fs::create_dir_all(path_profile.join("windata/AppData/LocalLow"))?;
    std::fs::create_dir_all(path_profile.join("windata/AppData/Roaming"))?;
    std::fs::create_dir_all(path_profile.join("windata/Documents"))?;
    std::fs::create_dir_all(path_profile.join("windata/Saved Games"))?;
    std::fs::create_dir_all(path_profile.join("windata/Desktop"))?;
    std::fs::create_dir_all(path_profile.join("home/.local/share"))?;
    std::fs::create_dir_all(path_profile.join("home/.config"))?;
    std::fs::create_dir_all(path_steam.clone())?;

    let usersettings = format!("[user::general]\naccount_name={name}");
    std::fs::write(path_steam.join("configs.user.ini"), usersettings)?;

    println!("[partydeck] Profile created successfully");
    Ok(())
}

// Creates the "game save" folder for per-profile game data to go into
pub fn create_profile_gamesave(name: &str, h: &Handler) -> Result<(), Box<dyn Error>> {
    let uid = h.handler_dir_name();
    let path_prof = PATH_PARTY.join("profiles").join(name);
    let path_gamesave = path_prof.join("gamesaves").join(&uid);
    let path_home = path_prof.join("home");
    let path_windata = path_prof.join("windata");

    if path_gamesave.exists() {
        return Ok(());
    }
    println!("[partydeck] Creating game save {} for {}", uid, name);

    std::fs::create_dir_all(&path_gamesave)?;
    
    if let Some(appid) = h.steam_appid && h.use_goldberg {
        let path_exec = path_gamesave.join(&h.exec);
        let path_execdir = path_exec.parent().ok_or_else(|| "couldn't get parent")?;
        if !path_execdir.exists() {
            std::fs::create_dir_all(&path_execdir)?;
        }
        std::fs::write(path_execdir.join("steam_appid.txt"), appid.to_string())?;
    }

    let profile_copy_gamesave = PathBuf::from(&h.path_handler).join("profile_copy_gamesave");
    if profile_copy_gamesave.exists() {
        copy_dir_recursive(&profile_copy_gamesave, &path_gamesave)?;
    }

    let profile_copy_home = PathBuf::from(&h.path_handler).join("profile_copy_home");
    if profile_copy_home.exists() {
        copy_dir_recursive(&profile_copy_home, &path_home)?;
    }

    let profile_copy_windata = PathBuf::from(&h.path_handler).join("profile_copy_windata");
    if profile_copy_windata.exists() {
        copy_dir_recursive(&profile_copy_windata, &path_windata)?;
    }

    println!("[partydeck] Profile save data created successfully");
    Ok(())
}

// Gets a vector of all available profiles.
// include_guest true for building the profile selector dropdown, false for the profile viewer.
pub fn scan_profiles(include_guest: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(PATH_PARTY.join("profiles")) {
        for entry in entries {
            if let Ok(entry) = entry
                && entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                && let Some(name) = entry.file_name().to_str()
            {
                out.push(name.to_string());
            }
        }
    }

    out.sort();

    if include_guest {
        out.insert(0, "Guest".to_string());
    }

    out
}

pub fn remove_guest_profiles() -> Result<(), Box<dyn Error>> {
    let path_profiles = PATH_PARTY.join("profiles");
    let entries = std::fs::read_dir(&path_profiles)?;
    for entry in entries.flatten() {
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with(".") {
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

pub static GUEST_NAMES: [&str; 33] = [
    "Blinky", "Pinky", "Inky", "Clyde", "Beatrice", "Battler", "Miyao", "Rena", "Ellie", "Joel",
    "Leon", "Ada", "Madeline", "Theo", "Yokatta", "Wyrm", "Brodiee", "Supreme", "Conk", "Gort",
    "Lich", "Smores", "Canary", "Trico", "Yorda", "Wander", "Agro", "Jak", "Daxter", "Soap",
    "Ghost", "Tomi", "Masaki",
];
