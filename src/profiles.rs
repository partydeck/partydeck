use std::error::Error;
use std::path::{Path, PathBuf};

use crate::{handler::Handler, paths::*, util::copy_dir_recursive};

/// SteamID64 base for individual accounts (account id = 0): 0x0110000100000000.
pub const STEAMID64_BASE: u64 = 76561197960265728;

/// A deterministic, valid SteamID64 derived from a profile name. Stable per
/// name, so a profile keeps the same Goldberg identity — and therefore the same
/// game save folder (e.g. `EldenRing/<SteamID>/ER0000.co2`) — across launches.
pub fn generate_steamid(name: &str) -> u64 {
    // FNV-1a (64-bit), a *fixed* hash. std's DefaultHasher is explicitly not
    // guaranteed stable across Rust/std versions, and this id becomes the
    // on-disk save-folder name — so a profile must derive the same SteamID
    // forever, even across a toolchain upgrade.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // account id is the low 32 bits; keep it non-zero so the id is well-formed.
    let account_id = (hash as u32).max(1) as u64;
    STEAMID64_BASE + account_id
}

/// Path to a profile's Goldberg user config (the per-profile `GseSavePath`
/// settings file that Goldberg reads `account_name`/`account_steamid` from).
pub fn profile_user_config_path(name: &str) -> PathBuf {
    PATH_PARTY.join(format!("profiles/{name}/steam/settings/configs.user.ini"))
}

/// Parse `account_steamid` out of a Goldberg `configs.user.ini` body. Tolerant
/// of the comments/blank lines Goldberg writes around it.
fn parse_account_steamid(contents: &str) -> Option<u64> {
    for line in contents.lines() {
        if let Some(val) = line.trim().strip_prefix("account_steamid=")
            && let Ok(id) = val.trim().parse::<u64>()
        {
            return Some(id);
        }
    }
    None
}

/// Read a profile's pinned Goldberg SteamID, if one has been written.
pub fn read_profile_steamid(name: &str) -> Option<u64> {
    parse_account_steamid(&std::fs::read_to_string(profile_user_config_path(name)).ok()?)
}

fn write_ini(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

/// Write a profile's Goldberg user config, pinning `account_steamid` so the
/// identity (and the game's save folder) is stable and reusable. Goldberg may
/// read this from either the per-profile `GseSavePath` settings or the default
/// `%APPDATA%/GSE Saves` location (which PartyDeck isolates per profile via
/// `windata`), so we write both — both are per-profile and harmless, and one
/// is whichever Goldberg actually reads. Preserves the account name.
pub fn write_profile_steamid(name: &str, steamid: u64) -> Result<(), std::io::Error> {
    let contents =
        format!("[user::general]\naccount_name={name}\naccount_steamid={steamid}\nlanguage=english\n");
    // Primary: GseSavePath/settings (must succeed).
    write_ini(&profile_user_config_path(name), &contents)?;
    // Reinforcement: the default %APPDATA%/GSE Saves location (best-effort).
    let _ = write_ini(
        &PATH_PARTY.join(format!(
            "profiles/{name}/windata/AppData/Roaming/GSE Saves/settings/configs.user.ini"
        )),
        &contents,
    );
    Ok(())
}

/// Ensure a profile has a pinned SteamID, generating + persisting one if it has
/// none yet (e.g. profiles created before this feature existed). Returns the id.
pub fn ensure_profile_steamid(name: &str) -> u64 {
    if let Some(id) = read_profile_steamid(name) {
        return id;
    }
    let id = generate_steamid(name);
    let _ = write_profile_steamid(name, id);
    id
}

/// Resolve a handler's `save_dir` template (with `$STEAMID`) to a profile's
/// actual on-disk save directory (under its per-profile `windata`, which
/// PartyDeck binds as the game's Windows user dir at launch).
pub fn profile_save_dir(profname: &str, save_dir_template: &str, steamid: u64) -> PathBuf {
    let rel = save_dir_template
        .replace('\\', "/")
        .replace("$STEAMID", &steamid.to_string());
    PATH_PARTY
        .join("profiles")
        .join(profname)
        .join("windata")
        .join(rel)
}

/// Whether a profile already holds a (non-empty) save for the given handler
/// save-dir template + SteamID.
pub fn profile_has_save(profname: &str, save_dir_template: &str, steamid: u64) -> bool {
    if save_dir_template.is_empty() {
        return false;
    }
    let dir = profile_save_dir(profname, save_dir_template, steamid);
    std::fs::read_dir(&dir)
        .map(|mut entries| entries.any(|e| e.is_ok()))
        .unwrap_or(false)
}

/// Copy an existing save file into a profile's save directory for the given
/// handler save-dir template + SteamID (creating the directory). Copy, not
/// symlink, so the original isn't mutated as the game writes saves. Returns the
/// destination path.
pub fn import_save_into_profile(
    profname: &str,
    save_dir_template: &str,
    steamid: u64,
    src: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    if save_dir_template.is_empty() {
        return Err("This game's handler has no save_dir set, so PartyDeck doesn't know where its saves live.".into());
    }
    let dir = profile_save_dir(profname, save_dir_template, steamid);
    std::fs::create_dir_all(&dir)?;
    let filename = src.file_name().ok_or("source path has no filename")?;
    let dest = dir.join(filename);
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

// Makes a folder and sets up Goldberg Steam Emu profile for Steam games
pub fn create_profile(name: &str) -> Result<(), std::io::Error> {
    if PATH_PARTY.join(format!("profiles/{name}")).exists() {
        // Existing profile: make sure it has a pinned SteamID even if it predates
        // this feature, so its saves stay stable and reusable.
        ensure_profile_steamid(name);
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

    // Pin a stable per-profile Goldberg SteamID (identity + save folder name).
    write_profile_steamid(name, generate_steamid(name))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_steamid_is_valid_and_distinct() {
        let a = generate_steamid("Tarnished_A");
        let b = generate_steamid("Tarnished_B");
        // Above the individual-account base, and deterministic per name.
        assert!(a > STEAMID64_BASE);
        assert!(b > STEAMID64_BASE);
        assert_eq!(a, generate_steamid("Tarnished_A"));
        assert_ne!(a, b);
        // 17-digit SteamID64, like a real one.
        assert_eq!(a.to_string().len(), 17);
    }

    #[test]
    fn parse_steamid_from_goldberg_ini() {
        // The exact shape Goldberg persists (comments + blanks around the value).
        let ini = "[user::general]\n\n# user account name\naccount_name=gse orca\n\n# Steam64 format\naccount_steamid=76561198328811168\n\nlanguage=english\n";
        assert_eq!(parse_account_steamid(ini), Some(76561198328811168));
        // Absent -> None.
        assert_eq!(parse_account_steamid("[user::general]\naccount_name=x\n"), None);
    }

    #[test]
    fn save_dir_substitutes_steamid_and_lands_under_windata() {
        let p = profile_save_dir("Anish", "AppData/Roaming/EldenRing/$STEAMID", 76561198007525187);
        let s = p.to_string_lossy();
        assert!(s.contains("/profiles/Anish/windata/AppData/Roaming/EldenRing/76561198007525187"));
        // Backslash templates are normalised too.
        let p2 = profile_save_dir("Anish", "AppData\\Local\\BET\\$STEAMID", 123);
        assert!(p2.to_string_lossy().contains("windata/AppData/Local/BET/123"));
    }

    #[test]
    fn has_save_false_when_no_template() {
        assert!(!profile_has_save("Anish", "", 1));
    }
}
