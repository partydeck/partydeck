use crate::paths::PATH_PARTY;

use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LastInstanceProfiles {
    #[serde(default)]
    pub profiles: Vec<String>,
}

fn state_path() -> PathBuf {
    PATH_PARTY.join("last_instance_profiles.json")
}

pub fn load_last_instance_profiles() -> LastInstanceProfiles {
    let path = state_path();

    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return LastInstanceProfiles::default();
        }
        Err(error) => {
            eprintln!("[partydeck] Failed to read {}: {}", path.display(), error);

            return LastInstanceProfiles::default();
        }
    };

    match serde_json::from_str(&content) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("[partydeck] Failed to parse {}: {}", path.display(), error);

            LastInstanceProfiles::default()
        }
    }
}

pub fn save_last_instance_profiles(
    state: &LastInstanceProfiles,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&*PATH_PARTY)?;

    let path = state_path();
    let temporary_path = path.with_extension("json.tmp");

    let content = serde_json::to_string_pretty(state)?;

    fs::write(&temporary_path, content)?;
    fs::rename(&temporary_path, &path)?;

    Ok(())
}
