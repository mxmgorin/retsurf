//! The static config-field table for the settings overlay: every editable
//! [`crate::config::AppConfig`] field as a [`Field`] row. A [`Kind`] bundles the
//! field's presentation with its typed get/set accessors into the config, so
//! adding a setting is adding one row to [`FIELDS`]. The Controls section is
//! not here — it's dynamic (see [`super::CtrlRow`]).

use super::SettingsSection;
use crate::config::{
    bounds, AppConfig, Channel, CursorMode, ExperimentalPreset, MemoryProfile, PageTheme,
    ToolbarPosition,
};

/// How a field is displayed, edited, and reached in a config. `Choice` carries
/// `(label, stored value)` pairs; `Int`/`Float` carry the bounds dpad steps
/// within (so the renderer and the adjust logic share one source of truth for
/// the range). The `get`/`set` fn pointers are the field's only binding to
/// [`AppConfig`].
pub enum Kind {
    Bool {
        get: fn(&AppConfig) -> bool,
        set: fn(&mut AppConfig, bool),
    },
    /// Free text, typed via the on-screen keyboard (Left/Right does nothing; A
    /// opens it). `get_mut` hands the OSK the draft's own buffer.
    Text {
        get: fn(&AppConfig) -> String,
        get_mut: fn(&mut AppConfig) -> &mut String,
    },
    Choice {
        opts: &'static [(&'static str, &'static str)],
        get: fn(&AppConfig) -> String,
        set: fn(&mut AppConfig, &str),
    },
    Int {
        min: i64,
        max: i64,
        step: i64,
        /// Label shown instead of a bare `0` (e.g. "Unlimited").
        zero: Option<&'static str>,
        get: fn(&AppConfig) -> i64,
        set: fn(&mut AppConfig, i64),
    },
    Float {
        min: f64,
        max: f64,
        step: f64,
        decimals: usize,
        get: fn(&AppConfig) -> f64,
        set: fn(&mut AppConfig, f64),
    },
}

/// `Kind::Bool` over a config path.
macro_rules! flag {
    ($($seg:ident).+) => {
        Kind::Bool {
            get: |c| c.$($seg).+,
            set: |c, v| c.$($seg).+ = v,
        }
    };
}

/// `Kind::Text` over a config `String` path.
macro_rules! text {
    ($($seg:ident).+) => {
        Kind::Text {
            get: |c| c.$($seg).+.clone(),
            get_mut: |c| &mut c.$($seg).+,
        }
    };
}

/// `Kind::Choice` over a `token_enum!` config path (`CHOICES` / `as_str` /
/// `from_value`).
macro_rules! choice {
    ($($seg:ident).+: $ty:ty) => {
        Kind::Choice {
            opts: <$ty>::CHOICES,
            get: |c| c.$($seg).+.as_str().to_string(),
            set: |c, v| c.$($seg).+ = <$ty>::from_value(v),
        }
    };
}

/// `Kind::Int` over a config path (cast to/from its native `$ty`), ranged by a
/// shared [`bounds::IntBounds`]; `step` is the GUI dpad step, the optional
/// trailing label replaces a bare `0`.
macro_rules! int {
    ($($seg:ident).+ as $ty:ty, $b:expr, $step:expr) => {
        int!($($seg).+ as $ty, $b, $step, None)
    };
    ($($seg:ident).+ as $ty:ty, $b:expr, $step:expr, $zero:expr) => {
        Kind::Int {
            min: $b.min,
            max: $b.max,
            step: $step,
            zero: $zero,
            get: |c| c.$($seg).+ as i64,
            set: |c, v| c.$($seg).+ = v as $ty,
        }
    };
}

/// `Kind::Float` over a config path (cast to/from its native `$ty`), ranged by
/// a shared [`bounds::FloatBounds`]; `step` is the GUI dpad step, `decimals`
/// the display precision.
macro_rules! float {
    ($($seg:ident).+ as $ty:ty, $b:expr, $step:expr, $decimals:expr) => {
        Kind::Float {
            min: $b.min,
            max: $b.max,
            step: $step,
            decimals: $decimals,
            get: |c| c.$($seg).+ as f64,
            set: |c, v| c.$($seg).+ = v as $ty,
        }
    };
}

/// A config row in the list. `section` is the tab it lives under; `cat` is a
/// sub-header shown only within sections that fold several config groups together
/// (see [`SettingsSection`]). `restart` marks fields the running app can't apply
/// live, flagged with `*` and a footer note. The Controls section is dynamic and
/// has no `Field`s — see [`super::CtrlRow`].
pub struct Field {
    pub section: SettingsSection,
    pub cat: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub restart: bool,
}

/// User-Agent presets: the keywords [`crate::config::BrowserConfig::user_agent`]
/// understands (empty keeps Servo's platform default).
const UA_CHOICES: &[(&str, &str)] = &[
    ("Default", ""),
    ("Desktop", "desktop"),
    ("Mobile", "mobile"),
    ("iOS", "ios"),
];

/// The User-Agent choice — a free `String` in the config, not a `token_enum!`:
/// a value outside [`UA_CHOICES`] shows verbatim and cycles back into the list
/// when adjusted.
const fn ua_kind() -> Kind {
    Kind::Choice {
        opts: UA_CHOICES,
        get: |c| c.browser.user_agent.clone(),
        set: |c, v| c.browser.user_agent = v.to_string(),
    }
}

/// The Web-features preset choice — derived from the experimental bools (shows
/// "Custom" when they match no preset); picking a preset rewrites all of them
/// (the bools are the source of truth, see [`ExperimentalPreset`]).
const fn web_features_kind() -> Kind {
    Kind::Choice {
        opts: ExperimentalPreset::CHOICES,
        get: |c| {
            ExperimentalPreset::detect(&c.experimental)
                .as_str()
                .to_string()
        },
        set: |c, v| c.experimental = ExperimentalPreset::from_value(v).features(),
    }
}

/// Compact constructor for the [`FIELDS`] table — without it `rustfmt` explodes
/// each `Field` literal across six lines and drowns the table.
const fn f(
    section: SettingsSection,
    cat: &'static str,
    label: &'static str,
    kind: Kind,
    restart: bool,
) -> Field {
    Field {
        section,
        cat,
        label,
        kind,
        restart,
    }
}

use SettingsSection as S;

/// Every editable config field, in display order (grouped by [`SettingsSection`]).
/// Adding a setting is adding a row here. `restart = true` marks fields the
/// running app can't apply live. The Controls section is not here — it's built
/// dynamically (see [`super::Settings::controls_rows`]).
#[rustfmt::skip]
pub(super) static FIELDS: &[Field] = &[
    f(S::Browser,  "Browser",     "Home page",              text!(browser.home_page), false),
    f(S::Browser,  "Browser",     "Search URL",             text!(browser.search_page), false),
    f(S::Browser,  "Browser",     "User agent",             ua_kind(), true),
    f(S::Browser,  "Browser",     "Page zoom",              float!(browser.page_zoom as f32, bounds::PAGE_ZOOM, 0.05, 2), false),
    f(S::Browser,  "Browser",     "Page theme",             choice!(browser.page_theme: PageTheme), false),
    f(S::Browser,  "Browser",     "Keep site data",         flag!(browser.persist_site_data), true),

    f(S::Browser,  "Experimental", "Web features",          web_features_kind(), false),
    f(S::Browser,  "Experimental", "WebGL 2",               flag!(experimental.webgl2), false),
    f(S::Browser,  "Experimental", "WebGPU",                flag!(experimental.webgpu), false),
    f(S::Browser,  "Experimental", "OffscreenCanvas",       flag!(experimental.offscreen_canvas), false),
    f(S::Browser,  "Experimental", "CSS Grid",              flag!(experimental.grid), false),
    f(S::Browser,  "Experimental", "CSS columns",           flag!(experimental.columns), false),
    f(S::Browser,  "Experimental", "Container queries",     flag!(experimental.container_queries), false),
    f(S::Browser,  "Experimental", "Web fonts",              flag!(experimental.fontface), false),
    f(S::Browser,  "Experimental", "IntersectionObserver",  flag!(experimental.intersection_observer), false),
    f(S::Browser,  "Experimental", "ResizeObserver",        flag!(experimental.resize_observer), false),
    f(S::Browser,  "Experimental", "Notifications",         flag!(experimental.notification), false),
    f(S::Browser,  "Experimental", "Async clipboard",       flag!(experimental.async_clipboard), false),
    f(S::Browser,  "Experimental", "Permissions",           flag!(experimental.permissions), false),

    f(S::Display,  "Display",     "Window width",           int!(display.width as u32, bounds::WIDTH, 16), true),
    f(S::Display,  "Display",     "Window height",          int!(display.height as u32, bounds::HEIGHT, 16), true),
    f(S::Display,  "Display",     "Use OpenGL ES",          flag!(display.use_gles), true),
    f(S::Display,  "Display",     "Cursor linger (ms)",     int!(display.cursor_linger_ms as u64, bounds::CURSOR_LINGER_MS, 100), false),
    f(S::Display,  "Display",     "Toolbar position",       choice!(display.toolbar_position: ToolbarPosition), false),
    f(S::Display,  "Display",     "Auto-hide toolbar",      flag!(display.toolbar_autohide), false),

    f(S::Input,  "Input",     "Stick dead zone",        float!(input.deadzone as f32, bounds::DEADZONE, 0.05, 2), false),
    f(S::Input,  "Input",     "Cursor speed",           float!(input.cursor_speed as f32, bounds::CURSOR_SPEED, 50.0, 0), false),
    f(S::Input,  "Input",     "Scroll speed",           float!(input.scroll_speed as f32, bounds::SCROLL_SPEED, 100.0, 0), false),
    f(S::Input,  "Input",     "Trigger threshold",      float!(input.trigger_threshold as f32, bounds::TRIGGER_THRESHOLD, 0.05, 2), false),
    f(S::Input,  "Input",     "OSK stick threshold",    float!(input.osk_nav_threshold as f32, bounds::OSK_NAV_THRESHOLD, 0.05, 2), false),
    f(S::Input,  "Input",     "OSK repeat delay (ms)",  int!(input.osk_nav_initial_delay_ms as u64, bounds::OSK_NAV_INITIAL_DELAY_MS, 50), false),
    f(S::Input,  "Input",     "OSK repeat rate (ms)",   int!(input.osk_nav_repeat_ms as u64, bounds::OSK_NAV_REPEAT_MS, 10), false),
    f(S::Input,  "Input",     "Hold gesture (ms)",      int!(input.hold_ms as u64, bounds::HOLD_MS, 50), false),
    f(S::Input,  "Input",     "Cursor mode",            choice!(input.cursor_mode: CursorMode), true),
    f(S::Input,  "Input",     "Hint badges",            flag!(input.hint_badges), false),

    f(S::Content,  "History",     "Record history",         flag!(history.enabled), false),
    f(S::Content,  "History",     "Max entries",            int!(history.max_entries as usize, bounds::HISTORY_MAX, 5), false),
    f(S::Content,  "Ad blocker",  "Enabled",                flag!(adblock.enabled), true),
    f(S::Content,  "Ad blocker",  "Update every (days)",    int!(adblock.update_days as u64, bounds::ADBLOCK_UPDATE_DAYS, 1), false),

    f(S::Content, "Data saving", "Block images",         flag!(data_saving.block_images), false),
    f(S::Content, "Data saving", "Block audio/video",    flag!(data_saving.block_media), false),
    f(S::Content, "Data saving", "Block web fonts",      flag!(data_saving.block_fonts), false),
    f(S::Content, "Data saving", "Max images/page",      int!(data_saving.max_images_per_page as usize, bounds::IMAGES_PER_PAGE, 8, Some("Unlimited")), false),

    f(S::Content, "Audio",       "Web Audio output",     flag!(audio.enabled), true),
    f(S::Content, "Audio",       "Max decode seconds",   int!(audio.max_decode_seconds as u32, bounds::DECODE_SECONDS, 30, Some("Unlimited")), true),

    f(S::Advanced, "Performance", "Memory profile",          choice!(performance.memory_profile: MemoryProfile), true),
    f(S::Advanced, "Performance", "Layout threads (0=auto)", int!(performance.layout_threads as u32, bounds::LAYOUT_THREADS, 1), true),
    f(S::Advanced, "Performance", "Worker pool max (0=auto)", int!(performance.worker_pool_max as u32, bounds::WORKER_POOL_MAX, 1), true),
    f(S::Advanced, "Performance", "HTTP disk cache (MB)",    int!(performance.http_disk_cache_mb as u32, bounds::HTTP_DISK_CACHE_MB, 8, Some("Off")), true),
    f(S::Advanced, "Downloads",   "Save folder",            text!(downloads.dir), true),
    f(S::Advanced, "Updates",     "Update channel",         choice!(update.channel: Channel), false),
    f(S::Advanced, "Updates",     "Auto-check on startup",  flag!(update.auto_check), false),
    f(S::Advanced, "Diagnostics", "Memory overlay",         flag!(debug.memory_overlay), false),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every accessor pair reads back what it wrote — catches a `get`/`set`
    /// wired to different config spots.
    #[test]
    fn accessors_roundtrip() {
        for field in FIELDS {
            let mut c = AppConfig::default();
            match &field.kind {
                Kind::Bool { get, set } => {
                    for v in [true, false] {
                        set(&mut c, v);
                        assert_eq!(get(&c), v, "{}", field.label);
                    }
                }
                Kind::Text { get, get_mut } => {
                    *get_mut(&mut c) = "roundtrip".to_string();
                    assert_eq!(get(&c), "roundtrip", "{}", field.label);
                }
                Kind::Choice { opts, get, set } => {
                    assert!(!opts.is_empty(), "{}", field.label);
                    for (_, token) in *opts {
                        set(&mut c, token);
                        assert_eq!(get(&c), *token, "{}", field.label);
                    }
                }
                Kind::Int {
                    min, max, get, set, ..
                } => {
                    for v in [*min, *max] {
                        set(&mut c, v);
                        assert_eq!(get(&c), v, "{}", field.label);
                    }
                }
                Kind::Float {
                    min, max, get, set, ..
                } => {
                    for v in [*min, *max] {
                        set(&mut c, v);
                        let got = get(&c);
                        assert!((got - v).abs() < 1e-3, "{}: {got} != {v}", field.label);
                    }
                }
            }
        }
    }
}
