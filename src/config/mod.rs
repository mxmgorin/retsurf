//! Application configuration: the `retsurf.toml` schema and its file I/O.
//!
//! [`AppConfig`] is the top-level aggregate, one field per `[section]` of the
//! TOML file; each section lives in its own submodule here. The GUI settings
//! screen ([`crate::overlay::settings`]) edits an `AppConfig` and writes it back
//! through [`AppConfig::save`]. Path/scale resolution shared across the crate
//! ([`data_dir`], [`device_scale`], …) lives in [`paths`].

use serde::{Deserialize, Serialize};

mod adblock;
pub mod bounds;
mod browser;
mod data_saving;
mod debug;
mod display;
mod downloads;
mod history;
mod input;
mod osk;
mod paths;
mod performance;
mod token_enum;

pub use adblock::AdblockConfig;
pub use browser::BrowserConfig;
pub use data_saving::DataSavingConfig;
pub use debug::DebugConfig;
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
    pub debug: DebugConfig,
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
        use bounds as b;

        fix_f32("browser.page_zoom", &mut self.browser.page_zoom, b::PAGE_ZOOM);

        fix_u32("display.width", &mut self.display.width, b::WIDTH);
        fix_u32("display.height", &mut self.display.height, b::HEIGHT);
        fix_u64(
            "display.cursor_linger_ms",
            &mut self.display.cursor_linger_ms,
            b::CURSOR_LINGER_MS,
        );

        let i = &mut self.input;
        fix_f32("input.deadzone", &mut i.deadzone, b::DEADZONE);
        fix_f32("input.cursor_speed", &mut i.cursor_speed, b::CURSOR_SPEED);
        fix_f32("input.scroll_speed", &mut i.scroll_speed, b::SCROLL_SPEED);
        fix_f32("input.trigger_threshold", &mut i.trigger_threshold, b::TRIGGER_THRESHOLD);
        fix_f32("input.osk_nav_threshold", &mut i.osk_nav_threshold, b::OSK_NAV_THRESHOLD);
        fix_u64(
            "input.osk_nav_initial_delay_ms",
            &mut i.osk_nav_initial_delay_ms,
            b::OSK_NAV_INITIAL_DELAY_MS,
        );
        fix_u64("input.osk_nav_repeat_ms", &mut i.osk_nav_repeat_ms, b::OSK_NAV_REPEAT_MS);
        fix_u64("input.hold_ms", &mut i.hold_ms, b::HOLD_MS);

        fix_usize("history.max_entries", &mut self.history.max_entries, b::HISTORY_MAX);
        fix_u64("adblock.update_days", &mut self.adblock.update_days, b::ADBLOCK_UPDATE_DAYS);
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

// Type-specific wrappers over `fix_ord` that cast the shared `i64` bounds to the
// field's own integer width. The bounds are small and non-negative, so the cast
// is exact.
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
    use super::{CursorMode, MemoryProfile, ToolbarPosition};

    /// Every `CHOICES` token round-trips through `from_value` -> `as_str`
    /// unchanged, and an unknown token falls back to the default — the lenient
    /// parse contract the GUI and hand-edited configs both rely on.
    fn check<T: PartialEq + Copy + std::fmt::Debug>(
        choices: &[(&str, &str)],
        from: impl Fn(&str) -> T,
        as_str: impl Fn(T) -> &'static str,
        default: T,
    ) {
        for (_, token) in choices {
            assert_eq!(as_str(from(token)), *token, "round-trip for `{token}`");
        }
        assert_eq!(from("not-a-real-token"), default);
    }

    #[test]
    fn config_enums_round_trip() {
        check(
            CursorMode::CHOICES,
            CursorMode::from_value,
            CursorMode::as_str,
            CursorMode::default(),
        );
        check(
            ToolbarPosition::CHOICES,
            ToolbarPosition::from_value,
            ToolbarPosition::as_str,
            ToolbarPosition::default(),
        );
        check(
            MemoryProfile::CHOICES,
            MemoryProfile::from_value,
            MemoryProfile::as_str,
            MemoryProfile::default(),
        );
    }

    #[test]
    fn from_value_is_lenient() {
        // Case- and whitespace-insensitive, unified across all three enums.
        assert_eq!(CursorMode::from_value("  SCROLL "), CursorMode::Scroll);
        assert_eq!(ToolbarPosition::from_value("Bottom"), ToolbarPosition::Bottom);
        assert_eq!(MemoryProfile::from_value(" Embedded"), MemoryProfile::Embedded);
    }
}
