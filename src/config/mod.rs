//! Application configuration: the `retsurf.toml` schema and its file I/O.
//!
//! [`AppConfig`] is the top-level aggregate, one field per `[section]` of the
//! TOML file; each section lives in its own submodule here. The GUI settings
//! screen ([`crate::overlay::settings`]) edits an `AppConfig` and writes it back
//! through [`AppConfig::save`]. Path/scale resolution shared across the crate
//! ([`data_dir`], [`device_scale`], …) lives in [`paths`].

use serde::{Deserialize, Serialize};

mod adblock;
mod browser;
mod data_saving;
mod display;
mod downloads;
mod history;
mod input;
mod osk;
mod paths;
mod performance;

pub use adblock::AdblockConfig;
pub use browser::BrowserConfig;
pub use data_saving::DataSavingConfig;
pub use display::{DisplayConfig, ToolbarPosition};
pub use downloads::DownloadsConfig;
pub use history::HistoryConfig;
pub use input::{CursorMode, InputConfig};
pub use osk::OskConfig;
pub use paths::{cache_dir, data_dir, device_scale, servo_data_dir};
pub use performance::{MemoryProfile, PerformanceConfig};

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
        let path = paths::config_path();
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
        self.write_to(&paths::config_path(), "config");
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
        fix_f32(
            "browser.page_zoom",
            &mut self.browser.page_zoom,
            0.3,
            3.0,
            1.0,
        );

        fix_ord("display.width", &mut self.display.width, 160, 3840);
        fix_ord("display.height", &mut self.display.height, 144, 2160);
        fix_ord(
            "display.cursor_linger_ms",
            &mut self.display.cursor_linger_ms,
            0,
            10_000,
        );

        let i = &mut self.input;
        fix_f32("input.deadzone", &mut i.deadzone, 0.0, 0.9, 0.25);
        fix_f32(
            "input.cursor_speed",
            &mut i.cursor_speed,
            100.0,
            3000.0,
            600.0,
        );
        fix_f32(
            "input.scroll_speed",
            &mut i.scroll_speed,
            100.0,
            5000.0,
            1600.0,
        );
        fix_f32(
            "input.trigger_threshold",
            &mut i.trigger_threshold,
            0.1,
            0.9,
            0.5,
        );
        fix_f32(
            "input.osk_nav_threshold",
            &mut i.osk_nav_threshold,
            0.1,
            0.9,
            0.5,
        );
        fix_ord(
            "input.osk_nav_initial_delay_ms",
            &mut i.osk_nav_initial_delay_ms,
            50,
            1000,
        );
        fix_ord("input.osk_nav_repeat_ms", &mut i.osk_nav_repeat_ms, 20, 500);
        fix_ord("input.hold_ms", &mut i.hold_ms, 100, 2000);

        fix_ord(
            "history.max_entries",
            &mut self.history.max_entries,
            0,
            1000,
        );
        fix_ord("adblock.update_days", &mut self.adblock.update_days, 0, 90);
        fix_ord(
            "performance.layout_threads",
            &mut self.performance.layout_threads,
            0,
            8,
        );
        fix_ord(
            "performance.worker_pool_max",
            &mut self.performance.worker_pool_max,
            0,
            16,
        );
    }
}

/// Clamp a float field into `[min, max]`, replacing a non-finite value with
/// `default`. Logs when it changes the stored value.
fn fix_f32(name: &str, v: &mut f32, min: f32, max: f32, default: f32) {
    let before = *v;
    *v = if v.is_finite() {
        v.clamp(min, max)
    } else {
        default
    };
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
