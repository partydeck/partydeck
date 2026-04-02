use crate::monitor::Monitor;
use crate::paths::{PATH_HOME, PATH_PARTY};

use dialog::{Choice, DialogBox};
use rfd::FileDialog;
use std::error::Error;
use std::os::fd::{AsFd, IntoRawFd};
use std::fs::{self, File};
use std::io::Read;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use nix::fcntl;
use nix::poll;
use nix::unistd;
use nix::unistd::Pid;
use std::env;
use std::os::fd::FromRawFd;

pub fn msg(title: &str, contents: &str) {
    let _ = dialog::Message::new(contents).title(title).show();
}

pub fn yesno(title: &str, contents: &str) -> bool {
    if let Ok(prompt) = dialog::Question::new(contents).title(title).show() {
        if prompt == Choice::Yes {
            return true;
        }
    }
    false
}

pub fn dir_dialog() -> Result<PathBuf, Box<dyn Error>> {
    let dir = FileDialog::new()
        .set_title("Select Folder")
        .set_directory(&*PATH_HOME)
        .pick_folder()
        .ok_or_else(|| "No folder selected")?;
    Ok(dir)
}

pub fn file_dialog_relative(base_dir: &PathBuf) -> Result<PathBuf, Box<dyn Error>> {
    let file = FileDialog::new()
        .set_title("Select File")
        .set_directory(base_dir)
        .pick_file()
        .ok_or_else(|| "No file selected")?;

    if file.starts_with(base_dir) {
        let relative_path = file.strip_prefix(base_dir)?;
        Ok(relative_path.to_path_buf())
    } else {
        Err("Selected file is not within the base directory".into())
    }
}

pub fn copy_dir_recursive(src: &PathBuf, dest: &PathBuf) -> Result<(), Box<dyn Error>> {
    println!(
        "[partydeck] util::copy_dir_recursive - src: {}, dest: {}",
        src.display(),
        dest.display()
    );

    let walk_path = walk_dir(src)?;

    for entry in walk_path {
        let rel_path = entry.strip_prefix(src)?;
        let new_path = dest.join(rel_path);

        if entry.is_symlink() {
            let symlink_src = std::fs::read_link(entry)?;
            std::os::unix::fs::symlink(symlink_src, new_path)?;
        } else if entry.is_dir() {
            std::fs::create_dir_all(&new_path)?;
        } else {
            if let Some(parent) = new_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if new_path.exists() {
                std::fs::remove_file(&new_path)?;
            }

            std::fs::copy(entry, new_path)?;
        }
    }

    Ok(())
}

pub fn walk_dir(path: &PathBuf) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    for entry in fs::read_dir(path)?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() && !path.is_symlink() {
            paths.push(path.clone());
            paths.extend(walk_dir(&path)?);
        } else {
            paths.push(path);
        }
    }

    Ok(paths)
}

pub fn zip_dir(src_dir: &PathBuf, dest: &PathBuf) -> Result<(), Box<dyn Error>> {
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    for entry in walk_dir(src_dir)? {
        let name = entry.strip_prefix(src_dir)?;
        if entry.is_symlink() {
            let target = fs::read_link(&entry)?;
            zip.add_symlink_from_path(name, target, options)?;
        } else if entry.is_dir() {
            zip.add_directory_from_path(name, options)?;
        } else {
            zip.start_file_from_path(name, options)?;
            io::copy(&mut fs::File::open(&entry)?, &mut zip)?;
        }
    }

    zip.finish()?;
    Ok(())
}

pub fn get_installed_steamapps() -> Vec<Option<steamlocate::App>> {
    let mut games = Vec::new();
    games.push(None);

    if let Ok(steam_dir) = steamlocate::SteamDir::locate()
        && let Ok(libraries) = steam_dir.libraries()
    {
        for library in libraries {
            let library = match library {
                Ok(lib) => lib,
                Err(_) => continue,
            };

            for app in library.apps() {
                if let Ok(app) = app {
                    games.push(Some(app));
                }
            }
        }
    }

    return games;
}

fn is_mount_point(dir: &PathBuf) -> Result<bool, Box<dyn std::error::Error>> {
    if let Ok(status) = Command::new("mountpoint").arg(dir).status()
        && status.success()
    {
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn fuse_overlayfs_unmount_gamedirs() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = PATH_PARTY.join("tmp");

    let Ok(entries) = std::fs::read_dir(&tmp) else {
        return Err("Failed to read directory".into());
    };

    for entry_result in entries {
        if let Ok(entry) = entry_result
            && entry.path().is_dir()
            && entry.file_name().to_string_lossy().starts_with("game-")
            && is_mount_point(&entry.path())?
        {
            let status = Command::new("umount")
                .arg("-l")
                .arg("-v")
                .arg(entry.path())
                .status()?;
            if !status.success() {
                return Err(format!("Unmounting {} failed", entry.path().to_string_lossy()).into());
            }
        }
    }

    Ok(())
}

pub fn clear_tmp() -> Result<(), Box<dyn Error>> {
    let tmp = PATH_PARTY.join("tmp");

    if !tmp.exists() {
        return Ok(());
    }

    fuse_overlayfs_unmount_gamedirs()?;

    std::fs::remove_dir_all(&tmp)?;

    Ok(())
}

pub fn check_for_partydeck_update() -> bool {
    // Try to get the latest release tag from GitHub
    if let Ok(client) = reqwest::blocking::Client::new()
        .get("https://api.github.com/repos/wunnr/partydeck/releases/latest")
        .header("User-Agent", "partydeck")
        .send()
    {
        if let Ok(release) = client.json::<serde_json::Value>() {
            // Extract the tag name (vX.X.X format)
            if let Some(tag_name) = release["tag_name"].as_str() {
                // Strip the 'v' prefix
                let latest_version = tag_name.strip_prefix('v').unwrap_or(tag_name);

                // Get current version from env!
                let current_version = env!("CARGO_PKG_VERSION");

                // Compare versions directly
                return latest_version != current_version;
            }
        }
    }

    // Default to false if any part of the process fails
    false
}

pub trait SanitizePath {
    fn sanitize_path(&self) -> String;
}

impl SanitizePath for String {
    fn sanitize_path(&self) -> String {
        if self.is_empty() {
            return String::new();
        }

        let mut sanitized = self.clone();

        // Remove potentially dangerous characters
        // Allow single quotes in paths since they are quoted when launching
        // commands. Double quotes would break the quoting though, so we still
        // strip those along with other potentially dangerous characters.
        let chars_to_sanitize = [';', '&', '|', '$', '`', '(', ')', '<', '>', '"', '\\', '/'];

        if chars_to_sanitize.iter().any(|&c| sanitized.contains(c)) {
            sanitized = sanitized
                .replace(";", "")
                .replace("&", "")
                .replace("|", "")
                .replace("$", "")
                .replace("`", "")
                .replace("(", "")
                .replace(")", "")
                .replace("<", "")
                .replace(">", "")
                .replace("\"", "")
                .replace("\\", "/") // Convert Windows backslashes to forward slashes
                .replace("//", "/"); // Remove any doubled slashes
        }

        // Prevent path traversal attacks
        while sanitized.contains("../") || sanitized.contains("./") {
            sanitized = sanitized.replace("../", "").replace("./", "");
        }

        // Remove leading slash to allow joining with other paths
        if sanitized.starts_with('/') {
            sanitized = sanitized[1..].to_string();
        }

        sanitized
    }
}

pub trait OsFmt {
    fn os_fmt(&self, win: bool) -> String;
}

impl OsFmt for String {
    fn os_fmt(&self, win: bool) -> String {
        if !win {
            return self.clone();
        } else {
            let path_fmt = self.replace("/", "\\");
            format!("Z:{}", path_fmt)
        }
    }
}

impl OsFmt for PathBuf {
    fn os_fmt(&self, win: bool) -> String {
        if !win {
            return self.to_string_lossy().to_string();
        } else {
            let path_fmt = self.to_string_lossy().replace("/", "\\");
            format!("Z:{}", path_fmt)
        }
    }
}

pub fn spawn_comp_and_get_display(
    comp_executable: &str,
    primary_monitor: Monitor,
) -> Option<(String, String, Monitor, Pid)> {
    let base_program_buf = env::current_exe().expect("Failed to get partydeck executable");
    let base_program = base_program_buf.to_str()?;

    let (read_fd, write_fd) = unistd::pipe().unwrap();

    let flags = fcntl::FdFlag::from_bits_truncate(
        fcntl::fcntl(&write_fd, fcntl::FcntlArg::F_GETFD).unwrap(),
    );
    fcntl::fcntl(
        &write_fd,
        fcntl::FcntlArg::F_SETFD(flags & !fcntl::FdFlag::FD_CLOEXEC),
    )
    .expect("Failed to open pipe to river");

    let mut cmd = std::process::Command::new(comp_executable);

    match comp_executable {
        "river" => {
            cmd.args([
                "-c",
                format!(
                    "{} --internal-layout {}:{}:{}",
                    base_program,
                    &write_fd.into_raw_fd(),
                    primary_monitor.width(),
                    primary_monitor.height()
                )
                .as_str(),
            ]);
        }
        "kwin_wayland" => {
            cmd.args([
                "--xwayland",
                "--exit-with-session",
                "--height",
                &primary_monitor.height().to_string(),
                "--width",
                &primary_monitor.width().to_string(),
                "--",
                format!(
                    "{} --internal-layout {}:{}:{}",
                    base_program,
                    &write_fd.into_raw_fd(),
                    primary_monitor.width(),
                    primary_monitor.height()
                )
                .as_str(),
            ]);
        }
        _ => {
            println!(
                "Unknown comp ({}); trusting that it works, MAY FAIL",
                comp_executable
            );
            cmd.args([
                "--",
                format!(
                    "{} --internal-layout {}:{}:{}",
                    base_program,
                    &write_fd.into_raw_fd(),
                    primary_monitor.width(),
                    primary_monitor.height()
                )
                .as_str(),
            ]);
        }
    }

    let child_pid;
    match cmd.spawn() {
        Ok(child) => child_pid = Pid::from_raw((child.id()) as i32),
        Err(e) => {
            eprintln!(
                "[partydeck] Failed to start COMP ({}): {}",
                comp_executable, e
            );
            return None;
        }
    }

    let mut fds = [poll::PollFd::new(read_fd.as_fd(), poll::PollFlags::POLLIN)];
    let res = poll::poll(&mut fds, 2000 as u16).unwrap_or(0);
    if res == 0 {
        eprintln!("[partydeck] NO DATA FROM COMP HANDLER");
        return None;
    }

    let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd.into_raw_fd()) };

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let way_disp_len = u32::from_be_bytes(len_buf) as usize;
    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let x11_disp_len = u32::from_be_bytes(len_buf) as usize;

    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let monitor_width = u32::from_be_bytes(len_buf);
    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let monitor_height = u32::from_be_bytes(len_buf);
    let remote_monitor = Monitor::new("REMOTE_MONITOR".to_owned(), monitor_width, monitor_height);

    let mut way_disp_buf = vec![0u8; way_disp_len];
    reader
        .read_exact(&mut way_disp_buf)
        .expect("Failed to read FD");
    let mut x11_disp_buf = vec![0u8; x11_disp_len];
    reader
        .read_exact(&mut x11_disp_buf)
        .expect("Failed to read FD");

    let way_disp = String::from_utf8(way_disp_buf).expect("Failed to decode FD");
    let x11_disp = String::from_utf8(x11_disp_buf).expect("Failed to decode FD");

    println!(
        "Got DISPLAY_WAYLAND from COMP: {} and DISPLAY: {}, with resolution: {}x{}",
        way_disp, x11_disp, monitor_width, monitor_height
    );

    return Some((
        way_disp.to_string(),
        x11_disp.to_string(),
        remote_monitor,
        child_pid,
    ));
}
