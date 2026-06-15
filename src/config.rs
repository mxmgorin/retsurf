use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub browser: BrowserConfig,
    pub display: DisplayConfig,
    pub input: InputConfig,
    pub history: HistoryConfig,
    pub downloads: DownloadsConfig,
    pub adblock: AdblockConfig,
    pub performance: PerformanceConfig,
    pub data_saving: DataSavingConfig,
    pub osk: OskConfig,
}

impl AppConfig {
    /// Load configuration from a TOML file. The path is `RETSURF_CONFIG` when set,
    /// otherwise `retsurf.toml` next to the executable (so a portable handheld
    /// install keeps everything in one folder regardless of working directory).
    /// A missing file yields defaults (and a template is written so it can be
    /// edited); a malformed file is logged and falls back to defaults.
    /// Unknown/omitted fields fall back to their defaults too, so a partial file
    /// (e.g. just `[input]`) is valid.
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(mut config) => {
                    config.sanitize();
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
        self.write_to(path, "default config");
    }

    /// Persist the current config to the config file — the GUI settings screen
    /// (see [`crate::overlay::settings`]) writes through here when it closes.
    /// Best-effort like [`Self::write_template`]: a failure is logged, not fatal,
    /// so the handheld's read-only-SD case degrades to in-memory-only changes.
    pub fn save(&self) {
        self.write_to(&config_path(), "config");
    }

    fn write_to(&self, path: &str, what: &str) {
        match toml::to_string_pretty(self) {
            Ok(text) => match std::fs::write(path, text) {
                Ok(()) => log::info!("wrote {what} to `{path}`"),
                Err(e) => log::warn!("could not write {what} `{path}`: {e}"),
            },
            Err(e) => log::warn!("could not serialize {what}: {e}"),
        }
    }

    /// Clamp hand-editable values to the same ranges the Settings GUI enforces
    /// (see [`crate::overlay::settings`]); a hand-edited file otherwise bypasses
    /// them, and an out-of-range value (e.g. `page_zoom = 0`, `width = 0`, a
    /// negative speed, or a NaN) can break rendering or input. Logs corrections.
    fn sanitize(&mut self) {
        fix_f32("browser.page_zoom", &mut self.browser.page_zoom, 0.3, 3.0, 1.0);

        fix_ord("display.width", &mut self.display.width, 160, 3840);
        fix_ord("display.height", &mut self.display.height, 144, 2160);
        fix_ord("display.cursor_linger_ms", &mut self.display.cursor_linger_ms, 0, 10_000);

        let i = &mut self.input;
        fix_f32("input.deadzone", &mut i.deadzone, 0.0, 0.9, 0.25);
        fix_f32("input.cursor_speed", &mut i.cursor_speed, 100.0, 3000.0, 600.0);
        fix_f32("input.scroll_speed", &mut i.scroll_speed, 100.0, 5000.0, 1600.0);
        fix_f32("input.trigger_threshold", &mut i.trigger_threshold, 0.1, 0.9, 0.5);
        fix_f32("input.osk_nav_threshold", &mut i.osk_nav_threshold, 0.1, 0.9, 0.5);
        fix_ord("input.osk_nav_initial_delay_ms", &mut i.osk_nav_initial_delay_ms, 50, 1000);
        fix_ord("input.osk_nav_repeat_ms", &mut i.osk_nav_repeat_ms, 20, 500);
        fix_ord("input.hold_ms", &mut i.hold_ms, 100, 2000);

        fix_ord("history.max_entries", &mut self.history.max_entries, 0, 1000);
        fix_ord("adblock.update_days", &mut self.adblock.update_days, 0, 90);
        fix_ord("performance.layout_threads", &mut self.performance.layout_threads, 0, 8);
        fix_ord("performance.worker_pool_max", &mut self.performance.worker_pool_max, 0, 16);
    }
}

/// Clamp a float field into `[min, max]`, replacing a non-finite value with
/// `default`. Logs when it changes the stored value.
fn fix_f32(name: &str, v: &mut f32, min: f32, max: f32, default: f32) {
    let before = *v;
    *v = if v.is_finite() { v.clamp(min, max) } else { default };
    if before.to_bits() != v.to_bits() {
        log::warn!("config: {name} = {before} out of range; using {}", *v);
    }
}

/// Clamp an ordered field into `[min, max]`. Logs when it changes the value.
fn fix_ord<T: PartialOrd + Copy + std::fmt::Display>(name: &str, v: &mut T, min: T, max: T) {
    let before = *v;
    if *v < min {
        *v = min;
    } else if *v > max {
        *v = max;
    }
    if *v != before {
        log::warn!("config: {name} = {before} out of range; using {}", *v);
    }
}

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
fn config_path() -> String {
    if let Ok(path) = std::env::var("RETSURF_CONFIG") {
        return path;
    }
    format!("{}config.toml", data_dir())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub home_page: String,
    pub experimental_prefs_enabled: bool,
    pub search_page: String,
    /// The User-Agent header sites see. Empty keeps Servo's platform default;
    /// the keywords `desktop`, `mobile` (or `android`), and `ios` pick the
    /// matching stock UA — `mobile` makes sites serve their phone layouts,
    /// which fit a small screen far better; anything else is sent verbatim.
    pub user_agent: String,
    /// Keep site data (cookies, localStorage, HSTS) across restarts, so logins
    /// survive. Stored in the `servo/` subfolder of the data dir
    /// (`cookie_jar.json`, `localstorage.json`, …). When false everything is
    /// in-memory only and gone on exit.
    pub persist_site_data: bool,
    /// Default page zoom for every tab (1.0 = 100%). Reflows the layout, so
    /// `1.25` makes the whole web bigger on a small screen; `zoom_in` /
    /// `zoom_out` step from here, `zoom_reset` returns.
    pub page_zoom: f32,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            // The built-in start page (crate::browser::home::HOME_URL).
            home_page: "retsurf:home".to_string(),
            experimental_prefs_enabled: true,
            search_page: "https://duckduckgo.com/?q=%s".to_string(),
            user_agent: String::new(),
            persist_site_data: true,
            page_zoom: 1.0,
        }
    }
}

/// Window/display settings (`[display]` in the config): size, GL backend, and
/// cursor-visibility timing.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub width: u32,
    pub height: u32,
    /// Request an OpenGL ES context (required on Mali handhelds) instead of
    /// desktop GL. Can be overridden at startup via `RETSURF_GLES=0`.
    pub use_gles: bool,
    /// How long the virtual cursor stays visible after the last movement, in ms.
    /// It hides when idle (nothing to hover) but lingers so you can see where it
    /// landed before clicking.
    pub cursor_linger_ms: u64,
}

impl Default for DisplayConfig {
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
#[derive(Clone, Serialize, Deserialize)]
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
            max_entries: 25,
        }
    }
}

/// Ad-blocker settings (`[adblock]` in the config): network-level filtering via
/// Brave's adblock-rust engine — see [`crate::browser::adblock`].
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdblockConfig {
    /// Master switch. When off, no lists are fetched and nothing is filtered.
    pub enabled: bool,
    /// Filter lists (EasyList syntax) downloaded into the engine.
    pub lists: Vec<String>,
    /// Re-download the lists once the cached engine is older than this many
    /// days; `0` never refreshes (keeps using whatever cache exists).
    pub update_days: u64,
}

impl Default for AdblockConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lists: vec![
                "https://easylist.to/easylist/easylist.txt".to_string(),
                "https://easylist.to/easylist/easyprivacy.txt".to_string(),
            ],
            update_days: 7,
        }
    }
}

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

/// On-screen-keyboard settings (`[osk]` in the config): which of the built-in
/// layouts are enabled — see [`crate::overlay::osk`] for the layout data itself.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OskConfig {
    /// Enabled layouts, in the order the keyboard's Lang key cycles them.
    /// Unknown names are logged and skipped; an empty (or fully invalid) list
    /// falls back to `["en"]`, so the keyboard always works.
    pub layouts: Vec<String>,
}

impl Default for OskConfig {
    fn default() -> Self {
        Self {
            layouts: vec!["en".to_string(), "ru".to_string()],
        }
    }
}

/// Servo thread-count tuning (`[performance]` in the config). Servo's defaults
/// assume a desktop (3 layout threads, worker pools of 4–6); on a 4-core
/// handheld that oversubscribes the cores, with the pools competing against
/// layout, script, and WebRender itself. `0` everywhere (the default) sizes
/// them from the machine's core count instead.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Memory/performance tier for the engine — JS heap ceilings, caches, and
    /// which DOM subsystems start (see [`MemoryProfile`]). `auto` (the default)
    /// picks one from the platform and detected RAM. The two thread knobs below
    /// override the tier's thread counts when set non-zero.
    pub memory_profile: MemoryProfile,
    /// Stylo/layout threads. `0` = keep the memory profile's choice; non-zero
    /// overrides it.
    pub layout_threads: u32,
    /// Cap on each of Servo's worker pools (image cache, async runtime,
    /// storage, WebRender workers). `0` = keep the memory profile's choice;
    /// non-zero overrides every pool with this value.
    pub worker_pool_max: u32,
}

/// Lightweight "data saving" mode (`[data_saving]` in the config): skip whole
/// subresource categories to cut bandwidth and memory. Each is blocked at the
/// network level like the ad blocker, so pages fail soft, and all apply live.
/// See [`crate::browser::content_filter`].
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DataSavingConfig {
    /// Skip image subresource loads (`<img>`, CSS backgrounds, favicons).
    pub block_images: bool,
    /// Skip audio/video/track media loads.
    pub block_media: bool,
    /// Skip web-font downloads — pages fall back to the bundled system fonts.
    pub block_fonts: bool,
}

/// Tunables for the gamepad-driven cursor, scroll, and on-screen-keyboard input,
/// plus the button bindings (see [`crate::event::bindings`]).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
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
    /// Holding a bound button this long fires its `hold:` gesture. The
    /// bindings themselves live in `bindings.toml` — see
    /// [`crate::event::bindings`].
    pub hold_ms: u64,
    /// Default D-pad/stick mode at startup ([`CursorMode`]). Toggle live with the
    /// `scroll` action; this only sets the initial mode.
    pub cursor_mode: CursorMode,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            deadzone: 0.25,
            cursor_speed: 600.0,
            scroll_speed: 1600.0,
            trigger_threshold: 0.5,
            osk_nav_threshold: 0.5,
            osk_nav_initial_delay_ms: 350,
            osk_nav_repeat_ms: 140,
            hold_ms: 400,
            cursor_mode: CursorMode::Mouse,
        }
    }
}

impl InputConfig {
    /// Whether the gamepad should start in scroll mode (vs the default cursor),
    /// per [`cursor_mode`](Self::cursor_mode).
    pub fn starts_in_scroll_mode(&self) -> bool {
        self.cursor_mode == CursorMode::Scroll
    }
}

/// The default behavior of the D-pad / left stick before any runtime toggle
/// (see the `scroll` action). Serializes to `"mouse"` / `"scroll"` in TOML.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorMode {
    /// Move a clickable on-screen cursor (the default).
    Mouse,
    /// Scroll the page.
    Scroll,
}

impl CursorMode {
    /// The TOML/UI token for this mode (`"mouse"` / `"scroll"`).
    pub fn as_str(self) -> &'static str {
        match self {
            CursorMode::Mouse => "mouse",
            CursorMode::Scroll => "scroll",
        }
    }

    /// Parse leniently: anything that isn't `"scroll"` (case-insensitive) is
    /// `Mouse`, so a typo can't break the config (mirrors `sanitize`'s clamping).
    pub fn from_value(s: &str) -> Self {
        if s.eq_ignore_ascii_case("scroll") {
            CursorMode::Scroll
        } else {
            CursorMode::Mouse
        }
    }
}

// Deserialize via a string so an unknown value falls back to `Mouse` instead of
// failing the whole config parse — the rest of the config degrades gracefully too.
impl<'de> Deserialize<'de> for CursorMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(&String::deserialize(d)?))
    }
}

/// Memory/performance tier for the Servo engine (`[performance] memory_profile`).
/// Each tier bundles a coordinated set of engine prefs — JS GC ceilings,
/// back-forward-cache depth, HTTP/canvas caches, thread counts, and which DOM
/// subsystems are even started — tuned for a class of hardware. Lower tiers
/// trade speed for a smaller footprint; `Auto` picks one from the build target
/// and detected RAM. See [`crate::browser::memory`] for what each tier sets.
/// Serializes to a lowercase token in TOML; an unknown value falls back to
/// `Auto`, like [`CursorMode`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProfile {
    /// Pick a tier from the build target and detected RAM (the default).
    #[default]
    Auto,
    /// Tightest floor (~512 MB, sub-1 GB boards): baseline JIT only, single
    /// thread, minimal caches, foreground tab only.
    Embedded,
    /// Very constrained (~1 GB boards): baseline JIT only, small caches.
    Tight,
    /// Balanced handheld (~2 GB boards): modest parallelism, full JIT.
    Balanced,
    /// Most headroom among handhelds (~4 GB): higher GC ceiling, modest threads.
    Generous,
    /// Android phone/tablet (>3 GB): full JIT, more threads, eager memory return.
    Android,
    /// Desktop / unconstrained: Servo's own defaults, untouched — no pref
    /// overrides and no thread clamp. The escape hatch when you want exactly
    /// what upstream ships.
    Desktop,
}

impl MemoryProfile {
    /// The TOML/UI token for this profile.
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryProfile::Auto => "auto",
            MemoryProfile::Embedded => "embedded",
            MemoryProfile::Tight => "tight",
            MemoryProfile::Balanced => "balanced",
            MemoryProfile::Generous => "generous",
            MemoryProfile::Android => "android",
            MemoryProfile::Desktop => "desktop",
        }
    }

    /// Parse leniently: an unrecognized token is `Auto`, so a typo can't break
    /// the config (mirrors `sanitize`'s clamping and [`CursorMode::from_value`]).
    pub fn from_value(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "embedded" => MemoryProfile::Embedded,
            "tight" => MemoryProfile::Tight,
            "balanced" => MemoryProfile::Balanced,
            "generous" => MemoryProfile::Generous,
            "android" => MemoryProfile::Android,
            "desktop" => MemoryProfile::Desktop,
            _ => MemoryProfile::Auto,
        }
    }
}

// Deserialize via a string so an unknown value falls back to `Auto` instead of
// failing the whole config parse (same rationale as `CursorMode`).
impl<'de> Deserialize<'de> for MemoryProfile {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(&String::deserialize(d)?))
    }
}
