use std::env;
use std::path::PathBuf;
use std::sync::LazyLock;

pub static PATH_RES: LazyLock<PathBuf> = LazyLock::new(|| {
    let localinstall = PathBuf::from("/usr/share/partydeck");
    if localinstall.exists() {
        return localinstall;
    }
    env::current_exe().unwrap().parent().unwrap().join("res")
});

pub static PATH_HOME: LazyLock<PathBuf> =
    LazyLock::new(|| PathBuf::from(env::var("HOME").unwrap()));

pub static PATH_LOCAL_SHARE: LazyLock<PathBuf> = LazyLock::new(|| PATH_HOME.join(".local/share"));

pub static PATH_PARTY: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("partydeck");
    }
    PATH_LOCAL_SHARE.join("partydeck")
});

pub static _PATH_STEAM: LazyLock<PathBuf> = LazyLock::new(|| {
    if PATH_HOME.join(".steam").exists() {
        PATH_HOME.join(".steam")
    } else if PATH_HOME
        .join(".var/app/com.valvesoftware.Steam/.steam/steam")
        .exists()
    {
        PATH_HOME.join(".var/app/com.valvesoftware.Steam/.steam/steam")
    } else {
        PATH_HOME.join(".steam")
    }
});

pub static BIN_UMU_RUN: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(umu_run) = pathsearch::find_executable_in_path("umu-run") {
        return umu_run;
    }

    let bin = env::current_exe().unwrap().parent().unwrap().join("bin");
    bin.join("umu-run")
});

pub static BIN_GSC_KBM: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(gsc_kbm) = pathsearch::find_executable_in_path("gamescope-kbm") {
        return gsc_kbm;
    }

    let bin = env::current_exe().unwrap().parent().unwrap().join("bin");
    bin.join("gamescope-kbm")
});
