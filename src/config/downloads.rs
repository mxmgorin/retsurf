use serde::{Deserialize, Serialize};

use super::data_dir;

/// File-download settings (`[downloads]` in the config). Servo has no download
/// support, so retsurf intercepts navigations to file-like URLs and fetches them
/// itself — see [`crate::data::downloads`].
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadsConfig {
    /// Where downloaded files are saved. Empty (the default) picks the system
    /// download folder (`XDG_DOWNLOAD_DIR` / `~/Downloads`) when one exists,
    /// otherwise `downloads/` inside the user data dir. Handheld installs can
    /// point this at the SD card (e.g. a ROMs folder).
    pub dir: String,
    /// URL path extensions (no dot) treated as downloads when navigated to: the
    /// navigation is cancelled and the file is fetched in the background instead.
    /// URLs without one of these extensions (e.g. dynamic `download.php?id=5`
    /// links) load in the browser as usual.
    pub extensions: Vec<String>,
}

impl Default for DownloadsConfig {
    fn default() -> Self {
        Self {
            dir: String::new(),
            extensions: [
                // archives
                "zip", "7z", "rar", "gz", "tgz", "bz2", "xz", "zst",
                // disc/flash images and packages
                "iso", "img", "bin", "cue", "chd", "pbp", "apk", "ipk", "deb", "rpm", "exe", "dmg",
                // documents servo can't render
                "pdf", // cartridge ROMs
                "nes", "sfc", "smc", "gba", "gbc", "gb", "nds", "n64", "z64", "v64", "smd", "gen",
                "32x", "sms", "gg", "pce", "ngp", "ngc", "ws", "wsc", "a26", "a78", "lnx", "vb",
                "rom",
            ]
            .map(str::to_string)
            .to_vec(),
        }
    }
}

impl DownloadsConfig {
    /// Resolve the save directory (with a trailing `/`): the configured one, else
    /// the system download folder, else `downloads/` in the user data dir.
    pub fn resolve_dir(&self) -> String {
        if !self.dir.is_empty() {
            return format!("{}/", self.dir.trim_end_matches('/'));
        }
        system_download_dir().unwrap_or_else(|| format!("{}downloads/", data_dir()))
    }
}

/// An explicit `RETSURF_DOWNLOAD_DIR` (e.g. set by the PortMaster launcher),
/// else the user's download folder per xdg-user-dirs (`XDG_DOWNLOAD_DIR` in
/// `user-dirs.dirs`), falling back to `~/Downloads`. `None` when none exist —
/// handhelds typically have no XDG dirs, desktops behave like a browser.
#[cfg(not(target_os = "android"))]
fn system_download_dir() -> Option<String> {
    // Explicit override wins over XDG autodetection; created on demand so a
    // launcher can point it at a fresh path (mirrors the Android branch).
    if let Ok(dir) = std::env::var("RETSURF_DOWNLOAD_DIR") {
        if !dir.is_empty() {
            match std::fs::create_dir_all(&dir) {
                Ok(()) => return Some(format!("{}/", dir.trim_end_matches('/'))),
                Err(e) => log::warn!("could not create RETSURF_DOWNLOAD_DIR `{dir}`: {e}"),
            }
        }
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| format!("{home}/.config"));
    let dirs = std::fs::read_to_string(format!("{config_home}/user-dirs.dirs")).unwrap_or_default();
    let mut dir = format!("{home}/Downloads");
    for line in dirs.lines() {
        // Shell-style assignment, e.g. `XDG_DOWNLOAD_DIR="$HOME/Downloads"`.
        if let Some(value) = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=") {
            let value = value.trim_matches('"').replace("$HOME", &home);
            if !value.is_empty() {
                dir = value;
            }
        }
    }
    std::path::Path::new(&dir)
        .is_dir()
        .then(|| format!("{}/", dir.trim_end_matches('/')))
}

/// Android has no XDG/`$HOME`; scoped storage means the writable, no-permission
/// location is the app-specific external dir. `RetsurfActivity` passes it (from
/// `getExternalFilesDir(DIRECTORY_DOWNLOADS)`) via `RETSURF_DOWNLOAD_DIR` — using
/// the Java API avoids `SDL_AndroidGetExternalStoragePath`, which isn't in
/// sdl2-sys's pregenerated bindings. `None` falls back to `downloads/` in the
/// internal data dir. (Files here are uninstall-scoped and not in the system
/// Downloads app; MediaStore/SAF visibility is a future enhancement.)
#[cfg(target_os = "android")]
fn system_download_dir() -> Option<String> {
    let dir = std::env::var("RETSURF_DOWNLOAD_DIR")
        .ok()
        .filter(|d| !d.is_empty())?;
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create RETSURF_DOWNLOAD_DIR `{dir}`: {e}");
        return None;
    }
    Some(format!("{}/", dir.trim_end_matches('/')))
}
