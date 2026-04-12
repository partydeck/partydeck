use std::fs;
use std::path::Path;

#[cfg(all(not(feature = "download_deps_latest"), feature = "download_deps"))]
use sha2::{Sha256, Digest};

#[cfg(feature = "download_deps")]
enum ArchFmt { TarBz2, Tar, SevenZ }

#[cfg(feature = "download_deps")]
struct Dep {
    repo: &'static str,
    asset_contains: &'static str,
    archive_name: &'static str,
    format: ArchFmt,
    #[allow(dead_code)]
    static_url: &'static str,
    #[allow(dead_code)]
    static_hash: &'static str,
    marker: &'static str,       // skip download if this exists
    rename_from: Option<&'static str>, // gbe extracts to "release/", rename it
}

#[cfg(feature = "download_deps")]
const DEPS: &[Dep] = &[
    Dep {
        repo: "Detanup01/gbe_fork",
        asset_contains: "emu-linux-release.tar.bz2",
        archive_name: "emu-linux-release.tar.bz2",
        format: ArchFmt::TarBz2,
        static_url: "https://github.com/Detanup01/gbe_fork/releases/download/release-2026_03_10/emu-linux-release.tar.bz2",
        static_hash: "032500ca100b72fd2daa94eec3263cf7c6fe63487623f9e28d70eee41a58bb01",
        marker: "gbe-linux/regular/x64/steamclient.so",
        rename_from: Some("gbe-linux"),
    },
    Dep {
        repo: "Detanup01/gbe_fork",
        asset_contains: "emu-win-release.7z",
        archive_name: "emu-win-release.7z",
        format: ArchFmt::SevenZ,
        static_url: "https://github.com/Detanup01/gbe_fork/releases/download/release-2026_03_10/emu-win-release.7z",
        static_hash: "0f67a4212aa4e6a71f84879a3a00f675cb2a8c43e13e38e0b27ab5c9e6a5e65f",
        marker: "gbe-win/steamclient_experimental/steamclient.dll",
        rename_from: Some("gbe-win"),
    },
    Dep {
        repo: "Open-Wine-Components/umu-launcher",
        asset_contains: "umu-launcher-",
        archive_name: "umu-launcher-latest-zipapp.tar",
        format: ArchFmt::Tar,
        static_url: "https://github.com/Open-Wine-Components/umu-launcher/releases/download/1.3.0/umu-launcher-1.3.0-zipapp.tar",
        static_hash: "36502de766f3cc549ff85196a04fb5afdb4eb2a72c023f22fd25895df91fda2f",
        marker: "umu/umu-run",
        rename_from: None,
    },
];

// (src relative to project root, dst relative to target dir)
const BUNDLE: &[(&str, &str)] = &[
    // goldberg linux
    ("deps/releases/gbe-linux/regular/x64/steamclient.so", "res/goldberg/linux64/steamclient.so"),
    ("deps/releases/gbe-linux/regular/x32/steamclient.so", "res/goldberg/linux32/steamclient.so"),
    // goldberg windows
    ("deps/releases/gbe-win/steamclient_experimental/steamclient.dll", "res/goldberg/win/steamclient.dll"),
    ("deps/releases/gbe-win/steamclient_experimental/steamclient64.dll", "res/goldberg/win/steamclient64.dll"),
    ("deps/releases/gbe-win/steamclient_experimental/GameOverlayRenderer.dll", "res/goldberg/win/GameOverlayRenderer.dll"),
    ("deps/releases/gbe-win/steamclient_experimental/GameOverlayRenderer64.dll", "res/goldberg/win/GameOverlayRenderer64.dll"),
    // umu
    ("deps/releases/umu/umu-run", "bin/umu-run"),
    // resources
    ("res/splitscreen_kwin.js", "res/splitscreen_kwin.js"),
    ("res/splitscreen_kwin_vertical.js", "res/splitscreen_kwin_vertical.js"),
];

const BUNDLE_OPTIONAL: &[(&str, &str)] = &[
    ("deps/gamescope/build-gcc/src/gamescope", "bin/gamescope-kbm"),
];


macro_rules! build_println {
    ($($arg:tt)*) => {
        println!("cargo:warning={}", format!($($arg)*));
    };
}

#[allow(dead_code)]
fn apply_patches(deps_dir: &std::path::Path) {
    let mut git_apply = std::process::Command::new("git");
    git_apply.args(["apply", &deps_dir.join("deps.patch").to_string_lossy()]);
    let _ = git_apply.spawn().map_err(|e| {
        build_println!("Failed to git apply the patches we have for our deps, this is most likely not a real error: {:?} - {:?}", git_apply.get_program().to_string_lossy(), e);
    });
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let deps_dir = root.join("deps/");
    fs::create_dir_all(&deps_dir).expect(&format!("failed to create directory: {:?}", deps_dir));

    #[cfg(feature = "download_deps")]
    for dep in DEPS {
        let releases_dir = deps_dir.join("releases/");
        fs::create_dir_all(&releases_dir).expect(&format!("failed to create directory: {:?}", releases_dir));

        fetch_dep(&releases_dir, dep).unwrap_or_else(|e| {
            panic!("failed to fetch {} from {}: {e}", dep.asset_contains, dep.repo);
        });
    }

    #[cfg(feature = "build_gamescope")]
    build_gamescope(&deps_dir);

    // cargo puts OUT_DIR a few levels deep, walk up to the profile dir (target/release/)
    let target_dir = Path::new(&std::env::var("OUT_DIR").unwrap())
        .ancestors().nth(3).unwrap().to_path_buf();

    for &(src, dst) in BUNDLE {
        let from = root.join(src);
        let to = target_dir.join(dst);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        if from.exists() {
            fs::copy(&from, &to).unwrap_or_else(|e| {
                build_println!("Continuing without it, but existing file for build {} failed to copy to {} due to: {e}", from.display(), to.display());
                0
            });
        } else {
            build_println!("Build skipping copying file {} due to it being inaccessable or not downloaded.", from.display());
        }
    }

    for &(src, dst) in BUNDLE_OPTIONAL {
        let from = root.join(src);
        if from.exists() {
            let to = target_dir.join(dst);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            let _ = fs::copy(&from, &to);
        }
    }
}

#[cfg(feature = "build_gamescope")]
fn build_gamescope(deps_dir: &Path) {
    apply_patches(deps_dir); // Apply our own custom fixes for gamescope compilation
    
    use std::process::Command;

    let gamescope_dir = deps_dir.join("gamescope");
    let build_dir = gamescope_dir.join("build-gcc");

    if !build_dir.exists() && gamescope_dir.exists() {
        build_println!("Running meson setup command for gamescope");
        let status = Command::new("meson")
            .arg("setup")
            .arg(&build_dir)
            .arg("-Dinput_emulation=disabled")
            .arg("-Dbenchmark=disabled")
            .arg("--auto-features=enabled")
            .env("CC", "gcc")
            .env("CXX", "g++")
            .current_dir(&gamescope_dir)
            .status()
            .expect("failed to run 'meson setup' for gamescope");
        assert!(status.success(), "'meson setup' failed for gamescope");
    }

    build_println!("Running ninja to compile gamescope - this can take a while.");
    let status = Command::new("ninja")
        .arg("-C")
        .arg(&build_dir)
        .status()
        .expect("failed to run ninja for gamescope");
    assert!(status.success(), "ninja build failed for gamescope");
}


#[cfg(all(not(feature = "download_deps_latest"), feature = "download_deps"))]
fn get_file_hash(file_path: &Path) -> String {
    let mut f = fs::File::open(file_path).unwrap();

    let mut h = Sha256::new();
    let mut buf = [0u8; 8192];

    while let Ok(n) = std::io::Read::read(&mut f, &mut buf) {
        if n == 0 { break; }
        h.update(&buf[..n]);
    }

    let hash = h.finalize();
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    
    hex
}

#[cfg(feature = "download_deps")]
fn fetch_dep(releases_dir: &Path, dep: &Dep) -> Result<(), Box<dyn std::error::Error>> {
    if releases_dir.join(dep.marker).exists() {
        return Ok(());
    }

    #[cfg(feature = "download_deps_latest")]
    let url = find_release_asset(dep.repo, dep.asset_contains)?;
    #[cfg(not(feature = "download_deps_latest"))]
    let url = dep.static_url;

    let archive = releases_dir.join(dep.archive_name);
    download(&url, &archive)?;

    let _ = fs::remove_dir_all(releases_dir.join("release"));
    if let Some(name) = dep.rename_from {
        let _ = fs::remove_dir_all(releases_dir.join(name));
    }

    #[cfg(all(not(feature = "download_deps_latest"), feature = "download_deps"))]
    {
        assert_eq!(get_file_hash(&archive), dep.static_hash);
        build_println!("Hash confirmed as {:?}", dep.static_hash);
    }

    match dep.format {
        ArchFmt::TarBz2 => {
            let f = fs::File::open(&archive)?;
            tar::Archive::new(bzip2::read::BzDecoder::new(f)).unpack(releases_dir)?;
        }
        ArchFmt::Tar => {
            let f = fs::File::open(&archive)?;
            tar::Archive::new(f).unpack(releases_dir)?;
        }
        ArchFmt::SevenZ => {
            sevenz_rust2::decompress_file(&archive, releases_dir)?;
        }
    }

    if let Some(name) = dep.rename_from {
        fs::rename(releases_dir.join("release"), releases_dir.join(name))?;
    }
    fs::remove_file(&archive)?;
    Ok(())
}

#[cfg(feature = "download_deps_latest")] 
fn find_release_asset(repo: &str, name_contains: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    build_println!("Downloading latest release URL for {repo}...");

    let resp: serde_json::Value = client
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("User-Agent", "partydeck-build")
        .send()?.error_for_status()?.json()?;

    for asset in resp["assets"].as_array().ok_or("no assets in release")? {
        let name = asset["name"].as_str().unwrap_or("");
        if name.contains(name_contains) {
            return Ok(asset["browser_download_url"].as_str()
                .ok_or("missing download url")?.to_string());
        }
    }
    Err(format!("no asset matching '{name_contains}' in {repo}").into())
}

#[cfg(feature = "download_deps")]
fn download(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();

    build_println!("Downloading latest release from {url} to file: {:?}...", dest);

    let mut resp = client.get(url)
        .header("User-Agent", "partydeck-build")
        .send()?.error_for_status()?;
    let mut file = fs::File::create(dest)?;
    std::io::copy(&mut resp, &mut file)?;
    Ok(())
}
