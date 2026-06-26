//! The static config-field table for the settings overlay: every editable
//! [`crate::config::AppConfig`] field as a [`Field`] row (presentation in
//! [`Kind`], identity in [`FieldId`]), plus the typed get/set dispatchers that
//! read and write the matching spot in a config by [`FieldId`]. The kind
//! metadata drives *how* a field is edited; the dispatchers drive *where*.
//! The Controls section is not here — it's dynamic (see [`super::CtrlRow`]).

use super::SettingsSection;
use crate::config::{bounds, AppConfig, CursorMode, MemoryProfile, ToolbarPosition};

/// One editable field, identified so the typed get/set helpers below can reach
/// the right spot in the draft without a parallel copy of the values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldId {
    HomePage,
    SearchPage,
    UserAgent,
    PageZoom,
    PersistSiteData,
    Width,
    Height,
    UseGles,
    CursorLinger,
    ToolbarPosition,
    ToolbarAutohide,
    Deadzone,
    CursorSpeed,
    ScrollSpeed,
    TriggerThreshold,
    OskNavThreshold,
    OskNavInitialDelay,
    OskNavRepeat,
    HoldMs,
    CursorMode,
    HintBadges,
    HistoryEnabled,
    HistoryMax,
    AdblockEnabled,
    AdblockUpdateDays,
    MemoryProfile,
    LayoutThreads,
    WorkerPoolMax,
    BlockImages,
    BlockMedia,
    BlockFonts,
    DownloadDir,
    MemoryOverlay,
}

/// How a field is displayed and edited. `Choice` carries `(label, stored value)`
/// pairs; `Int`/`Float` carry the bounds dpad steps within (so the renderer and the
/// adjust logic share one source of truth for the range).
pub enum Kind {
    Bool,
    /// Free text, typed via the on-screen keyboard (◀▶ does nothing; A opens it).
    Text,
    Choice(&'static [(&'static str, &'static str)]),
    Int {
        min: i64,
        max: i64,
        step: i64,
    },
    Float {
        min: f64,
        max: f64,
        step: f64,
        decimals: usize,
    },
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
    pub(super) id: FieldId,
    pub kind: Kind,
    pub restart: bool,
}

/// User-Agent presets: the keywords [`crate::config::BrowserConfig::user_agent`]
/// understands (empty keeps Servo's platform default). A UA set to something else
/// in the file shows verbatim and cycles back into this list when adjusted.
const UA_CHOICES: &[(&str, &str)] = &[
    ("Default", ""),
    ("Desktop", "desktop"),
    ("Mobile", "mobile"),
    ("iOS", "ios"),
];

/// Compact constructor for the [`FIELDS`] table — without it `rustfmt` explodes
/// each `Field` literal across six lines and drowns the table.
const fn f(
    section: SettingsSection,
    cat: &'static str,
    label: &'static str,
    id: FieldId,
    kind: Kind,
    restart: bool,
) -> Field {
    Field {
        section,
        cat,
        label,
        id,
        kind,
        restart,
    }
}

/// `Kind::Int` from a shared [`bounds::IntBounds`] range plus the GUI dpad step.
const fn int(b: bounds::IntBounds, step: i64) -> Kind {
    Kind::Int {
        min: b.min,
        max: b.max,
        step,
    }
}

/// `Kind::Float` from a shared [`bounds::FloatBounds`] range plus the GUI dpad
/// step and display precision.
const fn float(b: bounds::FloatBounds, step: f64, decimals: usize) -> Kind {
    Kind::Float {
        min: b.min,
        max: b.max,
        step,
        decimals,
    }
}

use FieldId as F;
use SettingsSection as S;

/// Every editable config field, in display order (grouped by [`SettingsSection`]).
/// Adding a setting is adding a row here plus an arm in the typed get/set helpers
/// below. `restart = true` marks fields the running app can't apply live. The
/// Controls section is not here — it's built dynamically (see [`super::Settings::controls_rows`]).
#[rustfmt::skip]
pub(super) static FIELDS: &[Field] = &[
    f(S::Browser,  "Browser",     "Home page",              F::HomePage,           Kind::Text, false),
    f(S::Browser,  "Browser",     "Search URL",             F::SearchPage,         Kind::Text, false),
    f(S::Browser,  "Browser",     "User agent",             F::UserAgent,          Kind::Choice(UA_CHOICES), true),
    f(S::Browser,  "Browser",     "Page zoom",              F::PageZoom,           float(bounds::PAGE_ZOOM, 0.05, 2), false),
    f(S::Browser,  "Browser",     "Keep site data",         F::PersistSiteData,    Kind::Bool, true),

    f(S::Display,  "Display",     "Window width",           F::Width,              int(bounds::WIDTH, 16), true),
    f(S::Display,  "Display",     "Window height",          F::Height,             int(bounds::HEIGHT, 16), true),
    f(S::Display,  "Display",     "Use OpenGL ES",          F::UseGles,            Kind::Bool, true),
    f(S::Display,  "Display",     "Cursor linger (ms)",     F::CursorLinger,       int(bounds::CURSOR_LINGER_MS, 100), false),
    f(S::Display,  "Display",     "Toolbar position",       F::ToolbarPosition,    Kind::Choice(ToolbarPosition::CHOICES), false),
    f(S::Display,  "Display",     "Auto-hide toolbar",      F::ToolbarAutohide,    Kind::Bool, false),

    f(S::Input,  "Input",     "Stick dead zone",        F::Deadzone,           float(bounds::DEADZONE, 0.05, 2), false),
    f(S::Input,  "Input",     "Cursor speed",           F::CursorSpeed,        float(bounds::CURSOR_SPEED, 50.0, 0), false),
    f(S::Input,  "Input",     "Scroll speed",           F::ScrollSpeed,        float(bounds::SCROLL_SPEED, 100.0, 0), false),
    f(S::Input,  "Input",     "Trigger threshold",      F::TriggerThreshold,   float(bounds::TRIGGER_THRESHOLD, 0.05, 2), false),
    f(S::Input,  "Input",     "OSK stick threshold",    F::OskNavThreshold,    float(bounds::OSK_NAV_THRESHOLD, 0.05, 2), false),
    f(S::Input,  "Input",     "OSK repeat delay (ms)",  F::OskNavInitialDelay, int(bounds::OSK_NAV_INITIAL_DELAY_MS, 50), false),
    f(S::Input,  "Input",     "OSK repeat rate (ms)",   F::OskNavRepeat,       int(bounds::OSK_NAV_REPEAT_MS, 10), false),
    f(S::Input,  "Input",     "Hold gesture (ms)",      F::HoldMs,             int(bounds::HOLD_MS, 50), false),
    f(S::Input,  "Input",     "Cursor mode",            F::CursorMode,         Kind::Choice(CursorMode::CHOICES), true),
    f(S::Input,  "Input",     "Hint badges",            F::HintBadges,         Kind::Bool, false),

    f(S::Content,  "History",     "Record history",         F::HistoryEnabled,     Kind::Bool, false),
    f(S::Content,  "History",     "Max entries",            F::HistoryMax,         int(bounds::HISTORY_MAX, 5), false),
    f(S::Content,  "Ad blocker",  "Enabled",                F::AdblockEnabled,     Kind::Bool, true),
    f(S::Content,  "Ad blocker",  "Update every (days)",    F::AdblockUpdateDays,  int(bounds::ADBLOCK_UPDATE_DAYS, 1), false),

    f(S::Content, "Data saving", "Block images",         F::BlockImages,        Kind::Bool, false),
    f(S::Content, "Data saving", "Block audio/video",    F::BlockMedia,         Kind::Bool, false),
    f(S::Content, "Data saving", "Block web fonts",      F::BlockFonts,         Kind::Bool, false),

    f(S::Advanced, "Performance", "Memory profile",          F::MemoryProfile,     Kind::Choice(MemoryProfile::CHOICES), true),
    f(S::Advanced, "Performance", "Layout threads (0=auto)", F::LayoutThreads,     int(bounds::LAYOUT_THREADS, 1), true),
    f(S::Advanced, "Performance", "Worker pool max (0=auto)", F::WorkerPoolMax,    int(bounds::WORKER_POOL_MAX, 1), true),
    f(S::Advanced, "Downloads",   "Save folder",            F::DownloadDir,        Kind::Text, true),
    f(S::Advanced, "Diagnostics", "Memory overlay",         F::MemoryOverlay,      Kind::Bool, false),
];

// --- Typed accessors into a config, keyed by `FieldId`. One arm per field; the
// kind metadata above drives *how* fields are edited, these drive *where*. ---

pub(super) fn get_num(c: &AppConfig, id: FieldId) -> f64 {
    match id {
        FieldId::PageZoom => c.browser.page_zoom as f64,
        FieldId::Width => c.display.width as f64,
        FieldId::Height => c.display.height as f64,
        FieldId::CursorLinger => c.display.cursor_linger_ms as f64,
        FieldId::Deadzone => c.input.deadzone as f64,
        FieldId::CursorSpeed => c.input.cursor_speed as f64,
        FieldId::ScrollSpeed => c.input.scroll_speed as f64,
        FieldId::TriggerThreshold => c.input.trigger_threshold as f64,
        FieldId::OskNavThreshold => c.input.osk_nav_threshold as f64,
        FieldId::OskNavInitialDelay => c.input.osk_nav_initial_delay_ms as f64,
        FieldId::OskNavRepeat => c.input.osk_nav_repeat_ms as f64,
        FieldId::HoldMs => c.input.hold_ms as f64,
        FieldId::HistoryMax => c.history.max_entries as f64,
        FieldId::AdblockUpdateDays => c.adblock.update_days as f64,
        FieldId::LayoutThreads => c.performance.layout_threads as f64,
        FieldId::WorkerPoolMax => c.performance.worker_pool_max as f64,
        _ => 0.0,
    }
}

pub(super) fn set_num(c: &mut AppConfig, id: FieldId, v: f64) {
    match id {
        FieldId::PageZoom => c.browser.page_zoom = v as f32,
        FieldId::Width => c.display.width = v as u32,
        FieldId::Height => c.display.height = v as u32,
        FieldId::CursorLinger => c.display.cursor_linger_ms = v as u64,
        FieldId::Deadzone => c.input.deadzone = v as f32,
        FieldId::CursorSpeed => c.input.cursor_speed = v as f32,
        FieldId::ScrollSpeed => c.input.scroll_speed = v as f32,
        FieldId::TriggerThreshold => c.input.trigger_threshold = v as f32,
        FieldId::OskNavThreshold => c.input.osk_nav_threshold = v as f32,
        FieldId::OskNavInitialDelay => c.input.osk_nav_initial_delay_ms = v as u64,
        FieldId::OskNavRepeat => c.input.osk_nav_repeat_ms = v as u64,
        FieldId::HoldMs => c.input.hold_ms = v as u64,
        FieldId::HistoryMax => c.history.max_entries = v as usize,
        FieldId::AdblockUpdateDays => c.adblock.update_days = v as u64,
        FieldId::LayoutThreads => c.performance.layout_threads = v as u32,
        FieldId::WorkerPoolMax => c.performance.worker_pool_max = v as u32,
        _ => {}
    }
}

pub(super) fn get_bool(c: &AppConfig, id: FieldId) -> bool {
    match id {
        FieldId::PersistSiteData => c.browser.persist_site_data,
        FieldId::UseGles => c.display.use_gles,
        FieldId::HistoryEnabled => c.history.enabled,
        FieldId::AdblockEnabled => c.adblock.enabled,
        FieldId::BlockImages => c.data_saving.block_images,
        FieldId::BlockMedia => c.data_saving.block_media,
        FieldId::BlockFonts => c.data_saving.block_fonts,
        FieldId::ToolbarAutohide => c.display.toolbar_autohide,
        FieldId::HintBadges => c.input.hint_badges,
        FieldId::MemoryOverlay => c.debug.memory_overlay,
        _ => false,
    }
}

pub(super) fn set_bool(c: &mut AppConfig, id: FieldId, b: bool) {
    match id {
        FieldId::PersistSiteData => c.browser.persist_site_data = b,
        FieldId::UseGles => c.display.use_gles = b,
        FieldId::HistoryEnabled => c.history.enabled = b,
        FieldId::AdblockEnabled => c.adblock.enabled = b,
        FieldId::BlockImages => c.data_saving.block_images = b,
        FieldId::BlockMedia => c.data_saving.block_media = b,
        FieldId::BlockFonts => c.data_saving.block_fonts = b,
        FieldId::ToolbarAutohide => c.display.toolbar_autohide = b,
        FieldId::HintBadges => c.input.hint_badges = b,
        FieldId::MemoryOverlay => c.debug.memory_overlay = b,
        _ => {}
    }
}

pub(super) fn get_text(c: &AppConfig, id: FieldId) -> &str {
    match id {
        FieldId::HomePage => &c.browser.home_page,
        FieldId::SearchPage => &c.browser.search_page,
        FieldId::DownloadDir => &c.downloads.dir,
        _ => "",
    }
}

pub(super) fn get_choice(c: &AppConfig, id: FieldId) -> &str {
    match id {
        FieldId::UserAgent => &c.browser.user_agent,
        FieldId::CursorMode => c.input.cursor_mode.as_str(),
        FieldId::MemoryProfile => c.performance.memory_profile.as_str(),
        FieldId::ToolbarPosition => c.display.toolbar_position.as_str(),
        _ => "",
    }
}

pub(super) fn set_choice(c: &mut AppConfig, id: FieldId, v: &str) {
    match id {
        FieldId::UserAgent => c.browser.user_agent = v.to_string(),
        FieldId::CursorMode => c.input.cursor_mode = CursorMode::from_value(v),
        FieldId::MemoryProfile => c.performance.memory_profile = MemoryProfile::from_value(v),
        FieldId::ToolbarPosition => c.display.toolbar_position = ToolbarPosition::from_value(v),
        _ => {}
    }
}
