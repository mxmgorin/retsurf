//! The static config-field table for the settings overlay: every editable
//! [`crate::config::AppConfig`] field as a [`Field`] row. A [`Kind`] bundles the
//! field's presentation with its typed get/set accessors into the config, so
//! adding a setting is adding one row to [`FIELDS`]. The binding list is not
//! here — it's dynamic, behind a [`Kind::Door`] row (see [`super::controls`]).

use super::SettingsSection;
use crate::config::{
    bounds, AppConfig, Channel, CursorMode, ExperimentalPreset, HomeStyle, MemoryProfile, OskStyle,
    PadLayout, PageTheme, Scaling, ToolbarPosition,
};
use crate::overlay::quick_access::Modes;

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
    /// A row that runs something instead of holding a value: the first A arms
    /// it, the second runs it (see [`super::Settings::confirm_action`]).
    Action { task: Task },
    /// A row that opens another screen.
    Door { door: Door },
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

/// The screen a [`Kind::Door`] row opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// The binding list (see [`super::controls`]).
    Bindings,
    /// Game Mode's input-map screens, which [`super::Settings::open_door`]
    /// leaves to its caller.
    InputMaps,
}

/// What a [`Kind::Action`] row runs; the app does the work (see
/// [`crate::app`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// Wipe history, site data, the HTTP cache, the saved session and the open
    /// tabs. Bookmarks, pins and the settings themselves stay.
    ClearData,
    /// Every settings row, the speed-dial pins and the control bindings back to
    /// how they ship. The user's own content is [`Task::ClearData`]'s business.
    RestoreDefaults,
}

impl Task {
    /// The verb shown as the row's value — what the second press will do.
    pub fn verb(self) -> &'static str {
        match self {
            Task::ClearData => "Clear",
            Task::RestoreDefaults => "Restore",
        }
    }
}

/// A row in a settings tab. `cat` is its sub-header, drawn only in a tab with
/// several (empty: none); `restart` marks a field the running app can't apply
/// live, flagged with `*`.
pub struct Field {
    pub section: SettingsSection,
    pub cat: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub restart: bool,
    /// Also a quick row, in these modes.
    pub quick: Option<Modes>,
    /// The quick row's label, where `label` leans on its sub-header.
    quick_label: Option<&'static str>,
}

impl Field {
    const fn quick(mut self, modes: Modes) -> Self {
        self.quick = Some(modes);
        self
    }

    const fn quick_as(mut self, modes: Modes, label: &'static str) -> Self {
        self.quick_label = Some(label);
        self.quick(modes)
    }

    /// What the row reads as out of its tab.
    pub fn quick_label(&self) -> &'static str {
        self.quick_label.unwrap_or(self.label)
    }
}

/// `kind`'s value in `config` as a row shows it; empty for a row with none.
pub fn value_of(kind: &Kind, config: &AppConfig) -> String {
    match kind {
        Kind::Action { task } => task.verb().to_string(),
        Kind::Door { .. } => String::new(),
        Kind::Bool { get, .. } => if get(config) { "On" } else { "Off" }.to_string(),
        Kind::Text { get, .. } => {
            let t = get(config);
            if t.is_empty() {
                "(default)".to_string()
            } else {
                t
            }
        }
        Kind::Choice { opts, get, .. } => {
            let cur = get(config);
            opts.iter()
                .find(|(_, v)| *v == cur)
                .map(|(label, _)| label.to_string())
                .unwrap_or(cur)
        }
        Kind::Int { zero, get, .. } => {
            let v = get(config);
            match zero {
                Some(label) if v == 0 => label.to_string(),
                _ => format!("{v}"),
            }
        }
        Kind::Float { decimals, get, .. } => format!("{:.*}", decimals, get(config)),
    }
}

/// Step `kind` in `config` by `dx` (-1 left, +1 right): toggle a bool, cycle a
/// choice, or step a number within its bounds. Rows holding no value ignore it.
pub fn step(kind: &Kind, config: &mut AppConfig, dx: i32) {
    match kind {
        Kind::Text { .. } | Kind::Action { .. } | Kind::Door { .. } => {}
        Kind::Bool { get, set } => {
            let v = !get(config);
            set(config, v);
        }
        Kind::Choice { opts, get, set } => {
            let cur = get(config);
            let n = opts.len() as i32;
            let idx = opts.iter().position(|(_, v)| *v == cur).unwrap_or(0) as i32;
            let next = (idx + dx).rem_euclid(n) as usize;
            set(config, opts[next].1);
        }
        Kind::Int {
            min,
            max,
            step,
            get,
            set,
            ..
        } => {
            let v = (get(config) + dx as i64 * step).clamp(*min, *max);
            set(config, v);
        }
        Kind::Float {
            min,
            max,
            step,
            get,
            set,
            ..
        } => {
            let v = (get(config) + dx as f64 * step).clamp(*min, *max);
            set(config, v);
        }
    }
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
        quick: None,
        quick_label: None,
    }
}

use SettingsSection as S;

/// Every editable config field, in display order (grouped by [`SettingsSection`]).
/// The binding list is not here: it is built dynamically behind a door row.
#[rustfmt::skip]
pub(super) static FIELDS: &[Field] = &[
    f(S::Browser,  "Browser",     "Home page",              text!(browser.home_page), false),
    f(S::Browser,  "Browser",     "Search URL",             text!(browser.search_page), false),
    f(S::Browser,  "Browser",     "User agent",             ua_kind(), true),
    f(S::Browser,  "Browser",     "Page zoom",              float!(browser.page_zoom as f32, bounds::PAGE_ZOOM, 0.05, 2), false),
    f(S::Browser,  "Browser",     "Page theme",             choice!(browser.page_theme: PageTheme), false).quick(Modes::Browser),
    f(S::Browser,  "Browser",     "Restore tabs",           flag!(browser.restore_tabs), false),
    f(S::Browser,  "Browser",     "Max tabs",               int!(browser.max_tabs as u32, bounds::MAX_TABS, 1, Some("Unlimited")), false),
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
    f(S::Browser,  "Experimental", "IndexedDB",             flag!(experimental.indexeddb), false),
    f(S::Browser,  "Experimental", "StorageManager",        flag!(experimental.storage_manager), false),
    f(S::Browser,  "Experimental", "Notifications",         flag!(experimental.notification), false),
    f(S::Browser,  "Experimental", "Async clipboard",       flag!(experimental.async_clipboard), false),
    f(S::Browser,  "Experimental", "Permissions",           flag!(experimental.permissions), false),

    f(S::Gaming, "Game Mode",   "Input map",              Kind::Door { door: Door::InputMaps }, false),
    f(S::Gaming, "Game Mode",   "View",                   choice!(game_mode.view.scaling: Scaling), false).quick(Modes::Game),

    f(S::Interface, "Interface",  "Interface scale",        float!(interface.scale as f32, bounds::SCALE, bounds::SCALE_STEP, 2), false),
    f(S::Interface, "Interface",  "Toolbar position",       choice!(interface.toolbar_position: ToolbarPosition), false),
    f(S::Interface, "Interface",  "Auto-hide toolbar",      flag!(interface.toolbar_autohide), false),
    f(S::Interface, "Interface",  "Home style",             choice!(interface.home_style: HomeStyle), false),
    f(S::Interface, "Interface",  "Page icons",             flag!(interface.page_icons), false),
    f(S::Interface, "Interface",  "Cursor linger (ms)",     int!(interface.cursor_linger_ms as u64, bounds::CURSOR_LINGER_MS, 100), false),

    // No sub-header: the door belongs to no group.
    f(S::Controls, "",            "Button bindings",        Kind::Door { door: Door::Bindings }, false),

    f(S::Controls, "Gamepad",     "Gamepad layout",         choice!(controls.pad_layout: PadLayout), false),
    f(S::Controls, "Gamepad",     "Swap A/B and X/Y",       flag!(controls.swap_face_buttons), false),
    f(S::Controls, "Gamepad",     "Stick dead zone",        float!(controls.deadzone as f32, bounds::DEADZONE, 0.05, 2), false),
    f(S::Controls, "Gamepad",     "Trigger threshold",      float!(controls.trigger_threshold as f32, bounds::TRIGGER_THRESHOLD, 0.05, 2), false),
    f(S::Controls, "Gamepad",     "Hold gesture (ms)",      int!(controls.hold_ms as u64, bounds::HOLD_MS, 50), false),
    f(S::Controls, "Gamepad",     "Gamepad rumble",         flag!(controls.haptics), false),

    f(S::Controls, "Cursor & scroll", "Cursor mode",        choice!(controls.cursor_mode: CursorMode), true),
    f(S::Controls, "Cursor & scroll", "Cursor speed",       float!(controls.cursor_speed as f32, bounds::CURSOR_SPEED, 50.0, 0), false),
    f(S::Controls, "Cursor & scroll", "Scroll speed",       float!(controls.scroll_speed as f32, bounds::SCROLL_SPEED, 100.0, 0), false),
    f(S::Controls, "Cursor & scroll", "Edge scrolling",     flag!(controls.edge_scroll), false),
    f(S::Controls, "Cursor & scroll", "Hint badges",        flag!(controls.hint_badges), false),

    f(S::Controls, "Keyboard",    "On-screen keyboard",     choice!(osk.style: OskStyle), false),
    #[cfg(target_os = "android")]
    f(S::Controls, "Keyboard",    "System keyboard",        flag!(controls.system_keyboard), false),
    f(S::Controls, "Keyboard",    "Stick threshold",        float!(controls.osk_nav_threshold as f32, bounds::OSK_NAV_THRESHOLD, 0.05, 2), false),
    f(S::Controls, "Keyboard",    "Repeat delay (ms)",      int!(controls.osk_nav_initial_delay_ms as u64, bounds::OSK_NAV_INITIAL_DELAY_MS, 50), false),
    f(S::Controls, "Keyboard",    "Repeat rate (ms)",       int!(controls.osk_nav_repeat_ms as u64, bounds::OSK_NAV_REPEAT_MS, 10), false),

    f(S::Content,  "History",     "Record history",         flag!(history.enabled), false),
    f(S::Content,  "History",     "Max entries",            int!(history.max_entries as usize, bounds::HISTORY_MAX, 5), false),
    f(S::Content,  "Ad blocker",  "Enabled",                flag!(adblock.enabled), false).quick_as(Modes::Browser, "Ad blocker"),
    f(S::Content,  "Ad blocker",  "Update every (days)",    int!(adblock.update_days as u64, bounds::ADBLOCK_UPDATE_DAYS, 1), false),

    f(S::Content, "Data saving", "Block images",         flag!(data_saving.block_images), false),
    f(S::Content, "Data saving", "Block audio/video",    flag!(data_saving.block_media), false),
    f(S::Content, "Data saving", "Block web fonts",      flag!(data_saving.block_fonts), false),
    f(S::Content, "Data saving", "Max images/page",      int!(data_saving.max_images_per_page as usize, bounds::IMAGES_PER_PAGE, 8, Some("Unlimited")), false),

    f(S::Content, "Audio",       "Audio output",         flag!(audio.enabled), true),
    f(S::Content, "Audio",       "Max decode seconds",   int!(audio.max_decode_seconds as u32, bounds::DECODE_SECONDS, 30, Some("Unlimited")), true),
    f(S::Content, "Video",       "Video playback",       flag!(video.enabled), true),

    f(S::System,   "Performance", "Memory profile",          choice!(performance.memory_profile: MemoryProfile), true),
    f(S::System,   "Performance", "Layout threads (0=auto)", int!(performance.layout_threads as u32, bounds::LAYOUT_THREADS, 1), true),
    f(S::System,   "Performance", "Worker pool max (0=auto)", int!(performance.worker_pool_max as u32, bounds::WORKER_POOL_MAX, 1), true),
    f(S::System,   "Performance", "CPU boost on load",       flag!(performance.cpu_boost_on_load), false),
    f(S::System,   "Performance", "HTTP disk cache (MB)",    int!(performance.http_disk_cache_mb as u32, bounds::HTTP_DISK_CACHE_MB, 8, Some("Off")), true),
    f(S::System,   "Performance", "Frame cap (fps)",        int!(performance.max_fps as u32, bounds::MAX_FPS, 5, Some("Uncapped")), false),
    f(S::System,   "Display",     "Window width",           int!(display.width as u32, bounds::WIDTH, 16), true),
    f(S::System,   "Display",     "Window height",          int!(display.height as u32, bounds::HEIGHT, 16), true),
    f(S::System,   "Display",     "Use OpenGL ES",          flag!(display.use_gles), true),
    f(S::System,   "Downloads",   "Save folder",            text!(downloads.dir), true),
    f(S::System,   "Data",        "Clear browsing data",    Kind::Action { task: Task::ClearData }, false),
    f(S::System,   "Updates",     "Update channel",         choice!(update.channel: Channel), false),
    f(S::System,   "Updates",     "Auto-check on startup",  flag!(update.auto_check), false),
    f(S::System,   "Diagnostics", "Memory overlay",         flag!(debug.memory_overlay), false),
    f(S::System,   "Diagnostics", "Memory to log",          flag!(debug.memory_log), false),
    // Last row: it rewrites every other one.
    f(S::System,   "Reset",       "Restore all defaults",   Kind::Action { task: Task::RestoreDefaults }, true),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A label names one row per tab; a repeat is a row listed twice.
    #[test]
    fn labels_are_unique_within_a_tab() {
        for (i, a) in FIELDS.iter().enumerate() {
            for b in &FIELDS[i + 1..] {
                let same_tab = a.section == b.section;
                assert!(!(same_tab && a.label == b.label), "{}", a.label);
            }
        }
    }

    /// Every accessor pair reads back what it wrote — catches a `get`/`set`
    /// wired to different config spots.
    #[test]
    fn accessors_roundtrip() {
        for field in FIELDS {
            let mut c = AppConfig::default();
            match &field.kind {
                // No accessors to check — these rows hold no config value.
                Kind::Action { .. } | Kind::Door { .. } => {}
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
