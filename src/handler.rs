use crate::paths::*;
use crate::util::*;

use eframe::egui::{self, ImageSource};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub const HANDLER_SPEC_CURRENT_VERSION: u16 = 3;

#[derive(Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SDL2Override {
    #[default]
    No,
    Srt,
    Sys,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Handler {
    // Members that are determined by context
    #[serde(skip)]
    pub path_handler: PathBuf,
    #[serde(skip)]
    pub img_paths: Vec<PathBuf>,

    pub name: String,
    pub author: String,
    pub version: String,
    pub info: String,
    #[serde(default)]
    pub spec_ver: u16,

    pub path_gameroot: String,
    pub runtime: String,
    pub exec: String,
    pub args: String,
    pub env: String,
    #[serde(default)]
    pub sdl2_override: SDL2Override,

    pub pause_between_starts: Option<f64>,

    pub use_goldberg: bool,
    pub steam_appid: Option<u32>,

    pub game_null_paths: Vec<String>,
}

impl Default for Handler {
    fn default() -> Self {
        Self {
            path_handler: PathBuf::new(),
            img_paths: Vec::new(),
            path_gameroot: String::new(),

            name: String::new(),
            author: String::new(),
            version: String::new(),
            info: String::new(),
            spec_ver: HANDLER_SPEC_CURRENT_VERSION,

            runtime: String::new(),
            exec: String::new(),
            args: String::new(),
            env: String::new(),
            sdl2_override: SDL2Override::No,

            pause_between_starts: None,

            use_goldberg: false,
            steam_appid: None,

            game_null_paths: Vec::new(),
        }
    }
}

impl Handler {
    pub fn from_json(json_path: &PathBuf) -> Result<Self, Box<dyn Error>> {
        let file = File::open(json_path)?;
        let mut handler = serde_json::from_reader::<_, Handler>(BufReader::new(file))?;

        handler.path_handler = json_path
            .parent()
            .ok_or_else(|| "Invalid path")?
            .to_path_buf();
        handler.img_paths = handler.get_imgs();

        for path in &mut handler.game_null_paths {
            *path = path.sanitize_path();
        }

        Ok(handler)
    }

    pub fn from_cli(path_exec: &str, args: &str) -> Self {
        let mut handler = Self::default();

        handler.path_gameroot = Path::new(path_exec)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap();
        handler.exec = Path::new(path_exec)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap();
        handler.args = args.to_string();

        handler
    }

    pub fn icon(&self) -> ImageSource<'_> {
        if self.path_handler.join("icon.png").exists() {
            format!("file://{}/icon.png", self.path_handler.display()).into()
        } else {
            egui::include_image!("../res/executable_icon.png")
        }
    }

    pub fn display(&self) -> &str {
        self.name.as_str()
    }

    pub fn display_clamp(&self) -> String {
        if self.name.len() > 25 {
            let out = format!("{}...", &self.name[..22]);
            out.clone()
        } else {
            self.name.clone()
        }
    }

    pub fn win(&self) -> bool {
        let extension: &str = Path::new(self.exec.as_str())
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();

        let lowercase_extension = extension.to_ascii_lowercase();
        lowercase_extension == "exe" || lowercase_extension == "bat" || lowercase_extension == "cmd"
    }

    pub fn is_saved_handler(&self) -> bool {
        !self.path_handler.as_os_str().is_empty()
    }

    pub fn handler_dir_name(&self) -> &str {
        self.path_handler
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
    }

    fn get_imgs(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let imgs_path = self.path_handler.join("imgs");

        let entries = match std::fs::read_dir(imgs_path) {
            Ok(entries) => entries,
            Err(_) => return out,
        };

        for entry_result in entries {
            if let Ok(entry) = entry_result
                && let Ok(file_type) = entry.file_type()
                && file_type.is_file()
                && let Some(path_str) = entry.path().to_str()
                && (path_str.ends_with(".png") || path_str.ends_with(".jpg"))
            {
                out.push(entry.path());
            }
        }

        out.sort();
        out
    }

    pub fn remove_handler(&self) -> Result<(), Box<dyn Error>> {
        if !self.is_saved_handler() {
            return Err("No handler directory to remove".into());
        }
        // TODO: Also return err if handler path exists but is not inside PATH_PARTY/handlers
        std::fs::remove_dir_all(self.path_handler.clone())?;

        Ok(())
    }

    pub fn get_game_rootpath(&self) -> Result<String, Box<dyn Error>> {
        if let Some(appid) = &self.steam_appid
            && let Some((app, library)) = steamlocate::SteamDir::locate()?
                .find_app(*appid)
                .ok()
                .flatten()
        {
            {
                let path = library.resolve_app_dir(&app);
                if path.exists() {
                    let pathstr = path.to_string_lossy().to_string();
                    return Ok(pathstr);
                }
            }
        }

        if !self.path_gameroot.is_empty() && Path::new(&self.path_gameroot).exists() {
            return Ok(self.path_gameroot.clone());
        }

        Err("Game root path not found".into())
    }

    pub fn save_to_json(&mut self) -> Result<(), Box<dyn Error>> {
        // If handler has no path, assume we're saving a newly created handler
        if !self.is_saved_handler() {
            if self.name.is_empty() {
                // If handler is based on a Steam game try to get the game's install dir name
                if let Some(appid) = self.steam_appid
                    && let Ok(dir) = steamlocate::SteamDir::locate()
                    && let Ok(Some((app, _))) = dir.find_app(appid)
                {
                    self.name = app.install_dir;
                } else {
                    return Err("Name cannot be empty".into());
                }
            }
            if !PATH_PARTY.join("handlers").join(&self.name).exists() {
                self.path_handler = PATH_PARTY.join("handlers").join(&self.name);
            } else {
                let mut i = 1;
                while PATH_PARTY
                    .join("handlers")
                    .join(&format!("{}-{}", self.name, i))
                    .exists()
                {
                    i += 1;
                }
                self.path_handler = PATH_PARTY
                    .join("handlers")
                    .join(&format!("{}-{}", self.name, i));
            }
        }

        if !self.path_handler.exists() {
            std::fs::create_dir_all(&self.path_handler)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(self.path_handler.join("handler.json"), json)?;

        Ok(())
    }

    pub fn export_pd2(&self) -> Result<(), Box<dyn Error>> {
        if self.name.is_empty() {
            return Err("Name cannot be empty".into());
        }

        let mut file = FileDialog::new()
            .set_title("Save file to:")
            .set_directory(&*PATH_HOME)
            .add_filter("PartyDeck Handler Package", &["pd2"])
            .save_file()
            .ok_or_else(|| "File not specified")?;

        if file.extension().is_none() || file.extension() != Some("pd2".as_ref()) {
            file.set_extension("pd2");
        }

        let tmpdir = PATH_PARTY.join("tmp");
        std::fs::create_dir_all(&tmpdir)?;

        copy_dir_recursive(&self.path_handler, &tmpdir)?;

        // Clear the rootpath before exporting so that users downloading it can set their own
        let mut handlerclone = self.clone();
        handlerclone.path_gameroot = String::new();
        // Overwrite the handler.json file with handlerclone
        let json = serde_json::to_string_pretty(&mut handlerclone)?;
        std::fs::write(tmpdir.join("handler.json"), json)?;

        if file.is_file() {
            std::fs::remove_file(&file)?;
        }

        zip_dir(&tmpdir, &file)?;
        clear_tmp()?;

        Ok(())
    }
}

pub fn scan_handlers() -> Vec<Handler> {
    let mut out: Vec<Handler> = Vec::new();
    let handlers_path = PATH_PARTY.join("handlers");

    let Ok(entries) = std::fs::read_dir(handlers_path) else {
        return out;
    };

    for entry_result in entries {
        if let Ok(entry) = entry_result
            && let Ok(file_type) = entry.file_type()
            && file_type.is_dir()
        {
            let json_path = entry.path().join("handler.json");
            if json_path.exists()
                && let Ok(handler) = Handler::from_json(&json_path)
            {
                out.push(handler);
            }
        }
    }
    out.sort_by(|a, b| a.display().to_lowercase().cmp(&b.display().to_lowercase()));
    out
}

pub fn import_pd2() -> Result<(), Box<dyn Error>> {
    let Some(file) = FileDialog::new()
        .set_title("Select File")
        .set_directory(&*PATH_HOME)
        .add_filter("PartyDeck Handler Package", &["pd2"])
        .pick_file()
    else {
        return Ok(());
    };

    if !file.exists() || !file.is_file() || file.extension().unwrap_or_default() != "pd2" {
        return Err("Handler not valid!".into());
    }

    let dir_handlers = PATH_PARTY.join("handlers");
    let dir_tmp = PATH_PARTY.join("tmp");
    if !dir_tmp.exists() {
        std::fs::create_dir_all(&dir_tmp)?;
    }

    let mut archive = zip::ZipArchive::new(File::open(&file)?)?;
    archive.extract(&dir_tmp)?;

    let handler_path = dir_tmp.join("handler.json");
    if !handler_path.exists() {
        clear_tmp()?;
        return Err("handler.json not found in archive".into());
    }

    // This is stupid..
    let mut fileclone = file.clone();
    fileclone.set_extension("");
    let name = fileclone
        .file_name()
        .ok_or_else(|| "No filename")?
        .to_string_lossy();

    let path = {
        if !dir_handlers.join(name.as_ref()).exists() {
            dir_handlers.join(name.as_ref())
        } else {
            let mut i = 1;
            while PATH_PARTY
                .join("handlers")
                .join(&format!("{}-{}", name, i))
                .exists()
            {
                i += 1;
            }
            dir_handlers.join(&format!("{}-{}", name, i))
        }
    };

    copy_dir_recursive(&dir_tmp, &path)?;
    clear_tmp()?;

    Ok(())
}
