use crate::paths::*;

use std::error::Error;
use std::fs::File;
use std::io::BufReader;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
pub enum PadFilterType {
    All,
    #[default]
    NoSteamInput,
    OnlySteamInput,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PartyConfig {
    #[serde(default = "default_true")]
    pub enable_kwin_script: bool,
    #[serde(default = "default_true")]
    pub gamescope_fix_lowres: bool,
    #[serde(default = "default_true")]
    pub gamescope_sdl_backend: bool,
    #[serde(default)]
    pub gamescope_force_grab_cursor: bool,
    #[serde(default = "default_true")]
    pub kbm_support: bool,
    #[serde(default)]
    pub proton_version: String,
    #[serde(default = "default_true")]
    pub proton_separate_pfxs: bool,
    #[serde(default = "default_true")]
    pub proton_wow64: bool,
    #[serde(default)]
    pub vertical_two_player: bool,
    #[serde(default)]
    pub pad_filter_type: PadFilterType,
    #[serde(default)]
    pub allow_multiple_instances_on_same_device: bool,
    #[serde(default = "default_true")]
    pub profile_unique_dirs: bool,
    #[serde(default)]
    pub disable_mount_gamedirs: bool,
    #[serde(default)]
    pub check_for_updates: bool,
}

impl Default for PartyConfig {
    fn default() -> Self {
        PartyConfig {
            enable_kwin_script: true,
            gamescope_fix_lowres: true,
            gamescope_sdl_backend: true,
            gamescope_force_grab_cursor: false,
            kbm_support: true,
            proton_version: "".to_string(),
            proton_separate_pfxs: true,
            proton_wow64: true,
            vertical_two_player: false,
            pad_filter_type: PadFilterType::NoSteamInput,
            allow_multiple_instances_on_same_device: false,
            profile_unique_dirs: true,
            disable_mount_gamedirs: false,
            check_for_updates: true,
        }
    }
}

/// Game-behavior settings a handler can override; unset fields keep the app-wide value.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ConfigOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_two_player: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_unique_dirs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_mount_gamedirs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamescope_fix_lowres: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamescope_force_grab_cursor: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton_separate_pfxs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton_wow64: Option<bool>,
}

impl PartyConfig {
    pub fn with_overrides(&self, o: &ConfigOverrides) -> PartyConfig {
        let mut cfg = self.clone();
        cfg.vertical_two_player = o.vertical_two_player.unwrap_or(cfg.vertical_two_player);
        cfg.profile_unique_dirs = o.profile_unique_dirs.unwrap_or(cfg.profile_unique_dirs);
        cfg.disable_mount_gamedirs = o.disable_mount_gamedirs.unwrap_or(cfg.disable_mount_gamedirs);
        cfg.gamescope_fix_lowres = o.gamescope_fix_lowres.unwrap_or(cfg.gamescope_fix_lowres);
        cfg.gamescope_force_grab_cursor = o.gamescope_force_grab_cursor.unwrap_or(cfg.gamescope_force_grab_cursor);
        cfg.proton_version = o.proton_version.clone().unwrap_or(cfg.proton_version);
        cfg.proton_separate_pfxs = o.proton_separate_pfxs.unwrap_or(cfg.proton_separate_pfxs);
        cfg.proton_wow64 = o.proton_wow64.unwrap_or(cfg.proton_wow64);
        cfg
    }
}

pub fn load_cfg() -> PartyConfig {
    let path = PATH_PARTY.join("settings.json");

    if let Ok(file) = File::open(path) {
        if let Ok(config) = serde_json::from_reader::<_, PartyConfig>(BufReader::new(file)) {
            return config;
        }
    }

    // Return default settings if file doesn't exist or has error
    return PartyConfig::default();
}

pub fn save_cfg(config: &PartyConfig) -> Result<(), Box<dyn Error>> {
    let path = PATH_PARTY.join("settings.json");
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, config)?;
    Ok(())
}
