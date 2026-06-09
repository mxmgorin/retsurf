use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub browser: BrowserConfig,
    pub interface: InterfaceConfig,
    pub gamepad: GamepadConfig,
    pub history: HistoryConfig,
    pub downloads: DownloadsConfig,
}

impl AppConfig {
    /// Load configuration from a TOML file. The path is `RETSURF_CONFIG` when set,
    /// otherwise `retsurf.toml` next to the executable (so a portable handheld
    /// install keeps everything in one folder regardless of working directory).
    /// A missing file yields defaults (and a template is written so it can be
    /// edited); a malformed file is logged and falls back to defaults.
    /// Unknown/omitted fields fall back to their defaults too, so a partial file
    /// (e.g. just `[gamepad]`) is valid.
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => {
                    log::info!("loaded config from `{path}`");
                    config
                }
                Err(e) => {
                    log::error!("invalid config `{path}`: {e}; using defaults");
                    Self::default()
                }
            },
            Err(_) => {
                let config = Self::default();
                config.write_template(&path);
                config
            }
        }
    }

    /// Best-effort write of the default config so the user has a file to edit.
    /// Failures (e.g. a read-only filesystem on the handheld) are non-fatal.
    fn write_template(&self, path: &str) {
        match toml::to_string_pretty(self) {
            Ok(text) => match std::fs::write(path, text) {
                Ok(()) => log::info!("wrote default config to `{path}`"),
                Err(e) => log::warn!("could not write default config `{path}`: {e}"),
            },
            Err(e) => log::warn!("could not serialize default config: {e}"),
        }
    }
}

/// The per-user data directory (with a trailing separator) where retsurf keeps
/// its writable files — config now, history/bookmarks/sessions later. Backed by
/// SDL's `SDL_GetPrefPath` (e.g. `~/.local/share/mxmgorin/retsurf/` on Linux),
/// which is guaranteed writable and created on demand. Falls back to the working
/// directory if SDL can't provide a pref path.
pub fn data_dir() -> String {
    match sdl2::filesystem::pref_path("mxmgorin", "retsurf") {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("could not resolve preferences directory ({e}); using working directory");
            String::new() // empty prefix => paths resolve relative to the cwd
        }
    }
}

/// Resolve the config file path: `RETSURF_CONFIG` if set, otherwise `config.toml`
/// inside [`data_dir`].
fn config_path() -> String {
    if let Ok(path) = std::env::var("RETSURF_CONFIG") {
        return path;
    }
    format!("{}config.toml", data_dir())
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub home_page: String,
    pub experimental_prefs_enabled: bool,
    pub search_page: String,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            home_page: "https://duckduckgo.com".to_string(),
            experimental_prefs_enabled: true,
            search_page: "https://duckduckgo.com/?q=%s".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceConfig {
    pub width: u32,
    pub height: u32,
    /// Request an OpenGL ES context (required on Mali handhelds) instead of
    /// desktop GL. Can be overridden at startup via `RETSURF_GLES=0`.
    pub use_gles: bool,
    /// How long the gamepad cursor stays visible after the last movement, in ms.
    /// It hides when idle (nothing to hover) but lingers so you can see where it
    /// landed before clicking.
    pub cursor_linger_ms: u64,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            use_gles: true,
            cursor_linger_ms: 1500,
        }
    }
}

/// Visit-history settings. Recording can be turned off entirely, and the cap on
/// how many entries are kept is configurable, both via `[history]` in the config.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Whether visited pages are recorded. When false, any existing history is
    /// still shown and can be cleared, but no new entries are added.
    pub enabled: bool,
    /// Maximum entries kept (most-recent-first); older ones are dropped past this.
    pub max_entries: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 200,
        }
    }
}

/// File-download settings (`[downloads]` in the config). Servo has no download
/// support, so retsurf intercepts navigations to file-like URLs and fetches them
/// itself — see [`crate::downloads`].
#[derive(Serialize, Deserialize)]
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
                "pdf",
                // cartridge ROMs
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

/// The user's download folder per xdg-user-dirs (`XDG_DOWNLOAD_DIR` in
/// `user-dirs.dirs`), falling back to `~/Downloads`. `None` when it doesn't
/// exist — handhelds typically have neither, desktops behave like a browser.
fn system_download_dir() -> Option<String> {
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

/// Tunables for the gamepad-driven cursor, scroll, and on-screen-keyboard input.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadConfig {
    /// Stick deflection below this (normalized 0..1) is treated as centered.
    pub deadzone: f32,
    /// Cursor speed at full stick deflection, logical px per second.
    pub cursor_speed: f32,
    /// Scroll speed at full stick deflection, device px per second.
    pub scroll_speed: f32,
    /// Trigger pull (normalized) above which L2/R2 count as pressed.
    pub trigger_threshold: f32,
    /// Stick deflection above which it counts as a directional OSK press.
    pub osk_nav_threshold: f32,
    /// Delay before the first auto-repeat of stick-driven OSK navigation, in ms.
    pub osk_nav_initial_delay_ms: u64,
    /// Interval between auto-repeats of stick-driven OSK navigation, in ms.
    pub osk_nav_repeat_ms: u64,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            deadzone: 0.25,
            cursor_speed: 750.0,
            scroll_speed: 1600.0,
            trigger_threshold: 0.5,
            osk_nav_threshold: 0.5,
            osk_nav_initial_delay_ms: 350,
            osk_nav_repeat_ms: 140,
        }
    }
}
