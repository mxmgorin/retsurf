//! Application configuration: the `retsurf.toml` schema and its file I/O.
//!
//! [`AppConfig`] is the top-level aggregate, one field per `[section]` of the
//! TOML file; each section lives in its own submodule here. The GUI settings
//! screen ([`crate::overlay::settings`]) edits an `AppConfig` and writes it back
//! through [`AppConfig::save`]. Path/scale resolution shared across the crate
//! ([`data_dir`], [`device_scale`], …) lives in [`paths`].

use serde::{Deserialize, Serialize};

mod adblock;
mod audio;
pub mod bounds;
mod browser;
mod data_saving;
mod debug;
mod display;
mod downloads;
mod experimental;
mod game_mode;
mod history;
mod input;
mod osk;
mod pad_layout;
mod paths;
mod performance;
mod token_enum;
mod update;
mod video;

pub use adblock::AdblockConfig;
pub use audio::AudioConfig;
pub use browser::{BrowserConfig, PageTheme};
pub use data_saving::DataSavingConfig;
pub use debug::DebugConfig;
pub use display::{DisplayConfig, ToolbarPosition};
pub use downloads::DownloadsConfig;
pub use experimental::{ExperimentalConfig, ExperimentalPreset};
pub use game_mode::GameModeConfig;
pub use history::HistoryConfig;
pub use input::{CursorMode, InputConfig};
pub use osk::{OskConfig, OskStyle};
pub use pad_layout::PadLayout;
pub use paths::{cache_dir, data_dir, device_scale, servo_data_dir};
pub use performance::{MemoryProfile, PerformanceConfig};
pub use update::{Channel, UpdateConfig};
pub use video::VideoConfig;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub browser: BrowserConfig,
    pub experimental: ExperimentalConfig,
    pub game_mode: GameModeConfig,
    pub display: DisplayConfig,
    pub input: InputConfig,
    pub history: HistoryConfig,
    pub downloads: DownloadsConfig,
    pub adblock: AdblockConfig,
    pub performance: PerformanceConfig,
    pub data_saving: DataSavingConfig,
    pub audio: AudioConfig,
    pub video: VideoConfig,
    pub osk: OskConfig,
    pub debug: DebugConfig,
    pub update: UpdateConfig,
}

/// A boolean `RETSURF_*` toggle: `None` when unset, otherwise any value but
/// `"0"` is on.
pub(crate) fn env_flag(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|v| v != "0")
}

impl AppConfig {
    /// Load configuration from a TOML file: `RETSURF_CONFIG` when set, otherwise
    /// `retsurf.toml` next to the executable, so a portable install keeps
    /// everything in one folder. Any missing or malformed part falls back.
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

    /// Persist the current config, as the settings screen does when it closes.
    /// Best-effort like [`Self::write_template`], so a read-only SD card degrades
    /// to in-memory-only changes.
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

    /// Clamp hand-editable values to the ranges the Settings GUI enforces: a
    /// hand-edited file bypasses them, and an out-of-range value (`page_zoom = 0`,
    /// a NaN) breaks rendering or input. Logs its corrections.
    pub(crate) fn sanitize(&mut self) {
        use bounds as b;

        fix_f32(
            "browser.page_zoom",
            &mut self.browser.page_zoom,
            b::PAGE_ZOOM,
        );
        fix_u32("browser.max_tabs", &mut self.browser.max_tabs, b::MAX_TABS);

        fix_u32("display.width", &mut self.display.width, b::WIDTH);
        fix_u32("display.height", &mut self.display.height, b::HEIGHT);
        fix_u32("display.max_fps", &mut self.display.max_fps, b::MAX_FPS);
        fix_f32("display.scale", &mut self.display.scale, b::SCALE);
        fix_u64(
            "display.cursor_linger_ms",
            &mut self.display.cursor_linger_ms,
            b::CURSOR_LINGER_MS,
        );

        let i = &mut self.input;
        fix_f32("input.deadzone", &mut i.deadzone, b::DEADZONE);
        fix_f32("input.cursor_speed", &mut i.cursor_speed, b::CURSOR_SPEED);
        fix_f32("input.scroll_speed", &mut i.scroll_speed, b::SCROLL_SPEED);
        fix_f32(
            "input.trigger_threshold",
            &mut i.trigger_threshold,
            b::TRIGGER_THRESHOLD,
        );
        fix_f32(
            "input.osk_nav_threshold",
            &mut i.osk_nav_threshold,
            b::OSK_NAV_THRESHOLD,
        );
        fix_u64(
            "input.osk_nav_initial_delay_ms",
            &mut i.osk_nav_initial_delay_ms,
            b::OSK_NAV_INITIAL_DELAY_MS,
        );
        fix_u64(
            "input.osk_nav_repeat_ms",
            &mut i.osk_nav_repeat_ms,
            b::OSK_NAV_REPEAT_MS,
        );
        fix_u64("input.hold_ms", &mut i.hold_ms, b::HOLD_MS);

        fix_usize(
            "history.max_entries",
            &mut self.history.max_entries,
            b::HISTORY_MAX,
        );
        fix_usize(
            "data_saving.max_images_per_page",
            &mut self.data_saving.max_images_per_page,
            b::IMAGES_PER_PAGE,
        );
        fix_u64(
            "adblock.update_days",
            &mut self.adblock.update_days,
            b::ADBLOCK_UPDATE_DAYS,
        );
        fix_u32(
            "audio.max_decode_seconds",
            &mut self.audio.max_decode_seconds,
            b::DECODE_SECONDS,
        );
        fix_u32(
            "performance.layout_threads",
            &mut self.performance.layout_threads,
            b::LAYOUT_THREADS,
        );
        fix_u32(
            "performance.worker_pool_max",
            &mut self.performance.worker_pool_max,
            b::WORKER_POOL_MAX,
        );
        fix_u32(
            "performance.http_disk_cache_mb",
            &mut self.performance.http_disk_cache_mb,
            b::HTTP_DISK_CACHE_MB,
        );
    }
}

/// Clamp a float field into the bounds' range, replacing a non-finite value with
/// the bounds' default. Logs when it changes the stored value.
fn fix_f32(name: &str, v: &mut f32, b: bounds::FloatBounds) {
    let before = *v;
    *v = if v.is_finite() {
        v.clamp(b.min as f32, b.max as f32)
    } else {
        b.default as f32
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

// Wrappers over `fix_ord` casting the shared `i64` bounds to the field's own
// width; the bounds are small and non-negative, so the cast is exact.
fn fix_u32(name: &str, v: &mut u32, b: bounds::IntBounds) {
    fix_ord(name, v, b.min as u32, b.max as u32);
}

fn fix_u64(name: &str, v: &mut u64, b: bounds::IntBounds) {
    fix_ord(name, v, b.min as u64, b.max as u64);
}

fn fix_usize(name: &str, v: &mut usize, b: bounds::IntBounds) {
    fix_ord(name, v, b.min as usize, b.max as usize);
}

#[cfg(test)]
mod tests {
    use super::{Channel, CursorMode, MemoryProfile, ToolbarPosition};

    // Per-variant round-trips live in each `token_enum!` invocation's own
    // generated test; this only covers what the macro cannot know.
    #[test]
    fn from_value_is_lenient() {
        // Case- and whitespace-insensitive, unified across all the enums.
        assert_eq!(CursorMode::from_value("  SCROLL "), CursorMode::Scroll);
        assert_eq!(
            ToolbarPosition::from_value("Bottom"),
            ToolbarPosition::Bottom
        );
        assert_eq!(
            MemoryProfile::from_value(" Embedded"),
            MemoryProfile::Embedded
        );
        assert_eq!(Channel::from_value(" Nightly "), Channel::Nightly);
        // `ci` is the retired spelling; configs written before the channel moved to
        // the nightly release must not silently fall back to stable.
        assert_eq!(Channel::from_value(" CI "), Channel::Nightly);
        assert_eq!(Channel::Nightly.as_str(), "nightly");
    }
}
