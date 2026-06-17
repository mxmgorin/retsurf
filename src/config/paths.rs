//! Path/directory and environment resolution shared across the crate: the UI
//! scale, the user data dir and its subfolders, and the config file path.

/// UI/content scale factor. The Android activity sets `RETSURF_SCALE` to the
/// display density (`DisplayMetrics.density`) before SDL starts, so the toolbar
/// and page render at a readable size on high-DPI phones instead of 1:1 pixels.
/// Desktop leaves it unset and stays at 1.0 — egui already derives HiDPI there
/// from the drawable/window ratio, which is 1:1 on Android. Applied to egui's
/// zoom factor and Servo's `hidpi_scale_factor`. Clamped to a sane range.
pub fn device_scale() -> f32 {
    std::env::var("RETSURF_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|s| s.is_finite() && *s > 0.0)
        .map(|s| s.clamp(0.5, 6.0))
        .unwrap_or(1.0)
}

/// The per-user data directory (with a trailing separator) where retsurf keeps
/// its writable files — config, history/bookmarks, cookies, the adblock cache.
/// `RETSURF_DATA_DIR` overrides it (created on demand — handy for a portable
/// install or to keep several profiles apart); otherwise it's SDL's
/// `SDL_GetPrefPath` (e.g. `~/.local/share/mxmgorin/retsurf/` on Linux), which
/// is guaranteed writable and created on demand. Falls back to the working
/// directory if SDL can't provide a pref path.
pub fn data_dir() -> String {
    if let Ok(dir) = std::env::var("RETSURF_DATA_DIR") {
        let dir = dir.trim_end_matches('/');
        if !dir.is_empty() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("could not create RETSURF_DATA_DIR `{dir}`: {e}");
            }
            return format!("{dir}/");
        }
    }
    match sdl2::filesystem::pref_path("mxmgorin", "retsurf") {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("could not resolve preferences directory ({e}); using working directory");
            String::new() // empty prefix => paths resolve relative to the cwd
        }
    }
}

/// Subdirectory of [`data_dir`] holding the site data Servo itself manages —
/// cookies, localStorage, HSTS. Kept separate from retsurf's own files (config,
/// history, bookmarks) so the data dir stays legible. Created on demand; passed
/// to Servo as its `config_dir` (see [`crate::browser`]).
pub fn servo_data_dir() -> String {
    let dir = format!("{}servo/", data_dir());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create servo data dir `{dir}`: {e}");
    }
    dir
}

/// Subdirectory of [`data_dir`] for regenerable cache files — currently the
/// adblock engine (`adblock.dat`). Safe to wipe: anything here is rebuilt or
/// re-downloaded on demand. Created on demand. See [`crate::browser::adblock`].
pub fn cache_dir() -> String {
    let dir = format!("{}cache/", data_dir());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create cache dir `{dir}`: {e}");
    }
    dir
}

/// Resolve the config file path: `RETSURF_CONFIG` if set, otherwise `config.toml`
/// inside [`data_dir`].
pub(super) fn config_path() -> String {
    if let Ok(path) = std::env::var("RETSURF_CONFIG") {
        return path;
    }
    format!("{}config.toml", data_dir())
}
