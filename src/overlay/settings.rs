//! The full-screen settings overlay opened with the ⚙ toolbar button (and the
//! bound `settings` gesture): the config fields that [`crate::config::AppConfig`]
//! exposes, editable with the gamepad, grouped into the same kind of tabbed
//! sections as the menu ([`crate::overlay::menu`]). It owns a *draft* config — a
//! clone of the live one taken on open — that the rows mutate; closing saves the
//! draft to disk and the app re-applies what can change live (see [`crate::app`]).
//!
//! Controls mirror the menu but free up ◀▶ for editing: L1/R1 (shoulders) switch
//! section, ▲▼ move between rows, ◀▶ adjust the focused value, A edits, B saves
//! and closes — all reachable without an analog stick. [`crate::ui::settings`]
//! renders it.

use crate::config::{AppConfig, CursorMode, MemoryProfile, ToolbarPosition};

/// A settings section — one tab in the bar, mirroring [`crate::overlay::menu`]'s
/// sections. A few [`config`](crate::config) groups are folded together so the
/// bar stays narrow (Content = history + ad-block + data saving, Advanced =
/// performance + downloads); within those the field's `cat` is shown as a
/// sub-header.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Browser,
    Display,
    Input,
    /// History recording, the ad blocker, and data-saving content blocking,
    /// presented under one tab — they remain separate config sections
    /// (`[history]`, `[adblock]`, `[data_saving]`), shown here as sub-groups.
    Content,
    Advanced,
    /// Read-only "about this build" tab — no editable fields; see [`about_info`].
    About,
}

impl SettingsSection {
    /// Left-to-right order of the section bar.
    pub const ALL: [SettingsSection; 6] = [
        SettingsSection::Browser,
        SettingsSection::Display,
        SettingsSection::Input,
        SettingsSection::Content,
        SettingsSection::Advanced,
        SettingsSection::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsSection::Browser => "Browser",
            SettingsSection::Display => "Display",
            SettingsSection::Input => "Input",
            SettingsSection::Content => "Content",
            SettingsSection::Advanced => "Advanced",
            SettingsSection::About => "About",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap()
    }
}

/// One editable field, identified so the typed get/set helpers below can reach
/// the right spot in the draft without a parallel copy of the values.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldId {
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
}

/// How a field is displayed and edited. `Choice` carries `(label, stored value)`
/// pairs; `Int`/`Float` carry the bounds ◀▶ steps within (so the renderer and the
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

/// A row in the list. `section` is the tab it lives under; `cat` is a sub-header
/// shown only within sections that fold several config groups together (see
/// [`SettingsSection`]). `restart` marks fields the running app can't apply live,
/// flagged with `*` and a footer note.
pub struct Field {
    pub section: SettingsSection,
    pub cat: &'static str,
    pub label: &'static str,
    id: FieldId,
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

/// Default D-pad/stick mode — the `(label, stored value)` pairs map to
/// [`crate::config::CursorMode`].
const CURSOR_MODE_CHOICES: &[(&str, &str)] = &[("Mouse", "mouse"), ("Scroll", "scroll")];

/// Toolbar edge — the `(label, stored value)` pairs map to
/// [`crate::config::ToolbarPosition`].
const TOOLBAR_POSITION_CHOICES: &[(&str, &str)] = &[("Top", "top"), ("Bottom", "bottom")];

/// Engine memory/performance tiers — the `(label, stored value)` pairs map to
/// [`crate::config::MemoryProfile`]. `Auto` picks one from the platform + RAM.
const MEMORY_PROFILE_CHOICES: &[(&str, &str)] = &[
    ("Auto", "auto"),
    ("Embedded", "embedded"),
    ("Tight", "tight"),
    ("Balanced", "balanced"),
    ("Generous", "generous"),
    ("Android", "android"),
    ("Desktop (Servo defaults)", "desktop"),
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

use FieldId as F;
use SettingsSection as S;

/// Every editable field, in display order (grouped by [`SettingsSection`]). Adding
/// a setting is adding a row here plus an arm in the typed get/set helpers below.
/// `restart = true` marks fields the running app can't apply live.
#[rustfmt::skip]
static FIELDS: &[Field] = &[
    f(S::Browser,  "Browser",     "Home page",              F::HomePage,           Kind::Text, false),
    f(S::Browser,  "Browser",     "Search URL",             F::SearchPage,         Kind::Text, false),
    f(S::Browser,  "Browser",     "User agent",             F::UserAgent,          Kind::Choice(UA_CHOICES), true),
    f(S::Browser,  "Browser",     "Page zoom",              F::PageZoom,           Kind::Float { min: 0.3, max: 3.0, step: 0.05, decimals: 2 }, false),
    f(S::Browser,  "Browser",     "Keep site data",         F::PersistSiteData,    Kind::Bool, true),

    f(S::Display,  "Display",     "Window width",           F::Width,              Kind::Int { min: 160, max: 3840, step: 16 }, true),
    f(S::Display,  "Display",     "Window height",          F::Height,             Kind::Int { min: 144, max: 2160, step: 16 }, true),
    f(S::Display,  "Display",     "Use OpenGL ES",          F::UseGles,            Kind::Bool, true),
    f(S::Display,  "Display",     "Cursor linger (ms)",     F::CursorLinger,       Kind::Int { min: 0, max: 10000, step: 100 }, false),
    f(S::Display,  "Display",     "Toolbar position",       F::ToolbarPosition,    Kind::Choice(TOOLBAR_POSITION_CHOICES), false),
    f(S::Display,  "Display",     "Auto-hide toolbar",      F::ToolbarAutohide,    Kind::Bool, false),

    f(S::Input,  "Input",     "Stick dead zone",        F::Deadzone,           Kind::Float { min: 0.0, max: 0.9, step: 0.05, decimals: 2 }, false),
    f(S::Input,  "Input",     "Cursor speed",           F::CursorSpeed,        Kind::Float { min: 100.0, max: 3000.0, step: 50.0, decimals: 0 }, false),
    f(S::Input,  "Input",     "Scroll speed",           F::ScrollSpeed,        Kind::Float { min: 100.0, max: 5000.0, step: 100.0, decimals: 0 }, false),
    f(S::Input,  "Input",     "Trigger threshold",      F::TriggerThreshold,   Kind::Float { min: 0.1, max: 0.9, step: 0.05, decimals: 2 }, false),
    f(S::Input,  "Input",     "OSK stick threshold",    F::OskNavThreshold,    Kind::Float { min: 0.1, max: 0.9, step: 0.05, decimals: 2 }, false),
    f(S::Input,  "Input",     "OSK repeat delay (ms)",  F::OskNavInitialDelay, Kind::Int { min: 50, max: 1000, step: 50 }, false),
    f(S::Input,  "Input",     "OSK repeat rate (ms)",   F::OskNavRepeat,       Kind::Int { min: 20, max: 500, step: 10 }, false),
    f(S::Input,  "Input",     "Hold gesture (ms)",      F::HoldMs,             Kind::Int { min: 100, max: 2000, step: 50 }, false),
    f(S::Input,  "Input",     "Cursor mode",            F::CursorMode,         Kind::Choice(CURSOR_MODE_CHOICES), true),
    f(S::Input,  "Input",     "Hint badges",            F::HintBadges,         Kind::Bool, false),

    f(S::Content,  "History",     "Record history",         F::HistoryEnabled,     Kind::Bool, false),
    f(S::Content,  "History",     "Max entries",            F::HistoryMax,         Kind::Int { min: 0, max: 1000, step: 5 }, false),
    f(S::Content,  "Ad blocker",  "Enabled",                F::AdblockEnabled,     Kind::Bool, true),
    f(S::Content,  "Ad blocker",  "Update every (days)",    F::AdblockUpdateDays,  Kind::Int { min: 0, max: 90, step: 1 }, false),

    f(S::Content, "Data saving", "Block images",         F::BlockImages,        Kind::Bool, false),
    f(S::Content, "Data saving", "Block audio/video",    F::BlockMedia,         Kind::Bool, false),
    f(S::Content, "Data saving", "Block web fonts",      F::BlockFonts,         Kind::Bool, false),

    f(S::Advanced, "Performance", "Memory profile",          F::MemoryProfile,     Kind::Choice(MEMORY_PROFILE_CHOICES), true),
    f(S::Advanced, "Performance", "Layout threads (0=auto)", F::LayoutThreads,     Kind::Int { min: 0, max: 8, step: 1 }, true),
    f(S::Advanced, "Performance", "Worker pool max (0=auto)", F::WorkerPoolMax,    Kind::Int { min: 0, max: 16, step: 1 }, true),
    f(S::Advanced, "Downloads",   "Save folder",            F::DownloadDir,        Kind::Text, true),
];

/// Settings overlay state: visibility, the working draft, the active section,
/// and the focused row.
pub struct Settings {
    visible: bool,
    /// The config being edited — a clone of the live one taken on [`Self::open`].
    /// Rows mutate this; the app reads it back on close to save and re-apply.
    draft: AppConfig,
    /// The active section (one tab of the bar).
    section: SettingsSection,
    /// Focused row, as an index into [`FIELDS`] (always within `section`).
    selected: usize,
}

impl Settings {
    pub fn new() -> Self {
        Self {
            visible: false,
            draft: AppConfig::default(),
            section: SettingsSection::Browser,
            selected: 0,
        }
    }

    /// All field descriptors, in display order (the renderer filters by section).
    pub fn fields() -> &'static [Field] {
        FIELDS
    }

    #[inline]
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Open the overlay, seeding the draft from the live config and focusing the
    /// first row of the first section.
    pub fn open(&mut self, config: &AppConfig) {
        self.draft = config.clone();
        self.section = SettingsSection::Browser;
        self.selected = 0;
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// The edited config (cloned out by the app on close to save + apply).
    pub fn draft(&self) -> AppConfig {
        self.draft.clone()
    }

    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    #[inline]
    pub fn section(&self) -> SettingsSection {
        self.section
    }

    /// Focus a row directly (clicking it), syncing the active section to it.
    pub fn set_selected(&mut self, i: usize) {
        if let Some(field) = FIELDS.get(i) {
            self.section = field.section;
            self.selected = i;
        }
    }

    /// Jump straight to a section (clicking its tab), focusing its first row.
    pub fn set_section(&mut self, section: SettingsSection) {
        self.section = section;
        self.selected = FIELDS
            .iter()
            .position(|f| f.section == section)
            .unwrap_or(0);
    }

    /// Switch the active section by `delta` (L1/R1; clamped, no wrap).
    pub fn switch_section(&mut self, delta: i32) {
        let last = SettingsSection::ALL.len() as i32 - 1;
        let i = (self.section.index() as i32 + delta).clamp(0, last) as usize;
        self.set_section(SettingsSection::ALL[i]);
    }

    /// Move the focus by `dy` rows within the active section (clamped, no wrap).
    pub fn move_sel(&mut self, dy: i32) {
        let rows = self.section_indices();
        let Some(pos) = rows.iter().position(|&g| g == self.selected) else {
            return;
        };
        let np = (pos as i32 + dy).clamp(0, rows.len() as i32 - 1) as usize;
        self.selected = rows[np];
    }

    /// Global [`FIELDS`] indices belonging to the active section, in order.
    fn section_indices(&self) -> Vec<usize> {
        FIELDS
            .iter()
            .enumerate()
            .filter(|(_, f)| f.section == self.section)
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether the active section is the read-only [`SettingsSection::About`]
    /// page: it has no [`FIELDS`], so A / ◀▶ are no-ops on it (and `selected`
    /// still points at some other section's row, which must not be touched).
    pub fn is_info_section(&self) -> bool {
        matches!(self.section, SettingsSection::About)
    }

    /// Whether the focused row holds free text (A opens the OSK on it). Always
    /// false on the About tab — there's nothing to type into.
    pub fn selected_is_text(&self) -> bool {
        !self.is_info_section() && matches!(FIELDS[self.selected].kind, Kind::Text)
    }

    /// Whether row `i` is numeric (the renderer shows ◀▶ step buttons for it).
    pub fn is_numeric(&self, i: usize) -> bool {
        matches!(FIELDS[i].kind, Kind::Int { .. } | Kind::Float { .. })
    }

    /// The OSK's edit buffer for the focused row — the draft's own `String` for a
    /// `Text` field, so typing lands straight in the draft. `None` for any other
    /// kind (the OSK only ever opens over a text row).
    pub fn selected_text_mut(&mut self) -> Option<&mut String> {
        let id = FIELDS[self.selected].id;
        let c = &mut self.draft;
        match id {
            FieldId::HomePage => Some(&mut c.browser.home_page),
            FieldId::SearchPage => Some(&mut c.browser.search_page),
            FieldId::DownloadDir => Some(&mut c.downloads.dir),
            _ => None,
        }
    }

    /// Adjust the focused field by `dx` (◀ = -1, ▶ = +1): toggle a bool, cycle a
    /// choice, or step a number within its bounds. Text rows ignore it.
    pub fn adjust(&mut self, dx: i32) {
        if self.is_info_section() {
            return;
        }
        let i = self.selected;
        let id = FIELDS[i].id;
        match &FIELDS[i].kind {
            Kind::Text => {}
            Kind::Bool => {
                let v = self.get_bool(id);
                self.set_bool(id, !v);
            }
            Kind::Choice(opts) => {
                let cur = self.get_choice(id);
                let n = opts.len() as i32;
                let idx = opts.iter().position(|(_, v)| *v == cur).unwrap_or(0) as i32;
                let next = (idx + dx).rem_euclid(n) as usize;
                self.set_choice(id, opts[next].1);
            }
            Kind::Int { min, max, step } => {
                let v =
                    (self.get_num(id) + dx as f64 * *step as f64).clamp(*min as f64, *max as f64);
                self.set_num(id, v.round());
            }
            Kind::Float { min, max, step, .. } => {
                let v = (self.get_num(id) + dx as f64 * step).clamp(*min, *max);
                self.set_num(id, v);
            }
        }
    }

    /// The display string for row `i`'s current value.
    pub fn value_str(&self, i: usize) -> String {
        let id = FIELDS[i].id;
        match &FIELDS[i].kind {
            Kind::Bool => if self.get_bool(id) { "On" } else { "Off" }.to_string(),
            Kind::Text => {
                let t = self.get_text(id);
                if t.is_empty() {
                    "(default)".to_string()
                } else {
                    t.to_string()
                }
            }
            Kind::Choice(opts) => {
                let cur = self.get_choice(id);
                opts.iter()
                    .find(|(_, v)| *v == cur)
                    .map(|(label, _)| label.to_string())
                    .unwrap_or_else(|| cur.to_string())
            }
            Kind::Int { .. } => format!("{}", self.get_num(id) as i64),
            Kind::Float { decimals, .. } => format!("{:.*}", decimals, self.get_num(id)),
        }
    }

    // --- Typed accessors into the draft, keyed by `FieldId`. One arm per field;
    // the kind metadata above drives *how* they're edited, these drive *where*. ---

    fn get_num(&self, id: FieldId) -> f64 {
        let c = &self.draft;
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

    fn set_num(&mut self, id: FieldId, v: f64) {
        let c = &mut self.draft;
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

    fn get_bool(&self, id: FieldId) -> bool {
        let c = &self.draft;
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
            _ => false,
        }
    }

    fn set_bool(&mut self, id: FieldId, b: bool) {
        let c = &mut self.draft;
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
            _ => {}
        }
    }

    fn get_text(&self, id: FieldId) -> &str {
        let c = &self.draft;
        match id {
            FieldId::HomePage => &c.browser.home_page,
            FieldId::SearchPage => &c.browser.search_page,
            FieldId::DownloadDir => &c.downloads.dir,
            _ => "",
        }
    }

    fn get_choice(&self, id: FieldId) -> &str {
        match id {
            FieldId::UserAgent => &self.draft.browser.user_agent,
            FieldId::CursorMode => self.draft.input.cursor_mode.as_str(),
            FieldId::MemoryProfile => self.draft.performance.memory_profile.as_str(),
            FieldId::ToolbarPosition => self.draft.display.toolbar_position.as_str(),
            _ => "",
        }
    }

    fn set_choice(&mut self, id: FieldId, v: &str) {
        match id {
            FieldId::UserAgent => self.draft.browser.user_agent = v.to_string(),
            FieldId::CursorMode => self.draft.input.cursor_mode = CursorMode::from_value(v),
            FieldId::MemoryProfile => {
                self.draft.performance.memory_profile = MemoryProfile::from_value(v)
            }
            FieldId::ToolbarPosition => {
                self.draft.display.toolbar_position = ToolbarPosition::from_value(v)
            }
            _ => {}
        }
    }
}

/// The read-only facts shown on the [`SettingsSection::About`] tab. Everything is
/// baked in at compile time by `build.rs` (see its docs): `version` is the crate
/// version, `git_hash`/`build_date` pin the source, and `components` are the
/// resolved versions of the headline dependencies. [`crate::ui::settings`] lays
/// it out; `credits` is the attribution block rendered below the table.
pub struct AboutInfo {
    pub version: &'static str,
    pub git_hash: &'static str,
    pub build_date: &'static str,
    /// Short blurb under the title — what retsurf is and how you drive it —
    /// rendered one dim line per entry.
    pub description: &'static [&'static str],
    /// `(display label, resolved version)`, in display order.
    pub components: &'static [(&'static str, &'static str)],
    /// Attribution / licensing lines, shown one per row under the table.
    pub credits: &'static [&'static str],
    /// Clickable `(label, url)` links shown below the credits; selecting one
    /// saves & closes the overlay and loads the URL (see
    /// [`crate::app::SettingsAction::OpenLink`]).
    pub links: &'static [(&'static str, &'static str)],
}

/// Build the About tab's content from the `RETSURF_*` env vars `build.rs` emits.
pub fn about_info() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_hash: env!("RETSURF_GIT_HASH"),
        build_date: env!("RETSURF_BUILD_DATE"),
        description: &[
            "Lightweight web browser powered by the Servo engine.",
            "Full gamepad control: virtual cursor, link hints, on-screen keyboard.",
            "Keyboard, mouse, and touch too — runs on handhelds, desktop, and Android.",
        ],
        components: &[
            ("Servo engine", env!("RETSURF_VER_SERVO")),
            ("egui", env!("RETSURF_VER_EGUI")),
            ("surfman", env!("RETSURF_VER_SURFMAN")),
            ("SDL2", env!("RETSURF_VER_SDL2")),
        ],
        credits: &[
            "Web rendering by the Servo project (MPL-2.0).",
            "UI by egui (MIT OR Apache-2.0).",
            "Windowing & input by SDL2 (zlib).",
            "retsurf is licensed under GPL-3.0.",
        ],
        links: &[
            ("Servo", "https://servo.org"),
            ("egui", "https://egui.rs"),
            ("SDL", "https://libsdl.org"),
            ("Source code", "https://github.com/mxmgorin/retsurf"),
        ],
    }
}
