//! The full-screen settings overlay opened with the ⚙ toolbar button (and the
//! bound `settings` gesture): the config fields that [`crate::config::AppConfig`]
//! exposes, editable with the gamepad, grouped into the same kind of tabbed
//! sections as the menu ([`crate::overlay::menu`]). It owns a *draft* config — a
//! clone of the live one taken on open — that the rows mutate; closing saves the
//! draft to disk and the app re-applies what can change live (see [`crate::app`]).
//!
//! Controls mirror the menu but free up dpad for editing: L1/R1 (shoulders) switch
//! section, up/down move between rows, left adjust the focused value, A edits, B saves
//! and closes — all reachable without an analog stick. The Controls section is
//! the exception: an action list where A *adds* a binding (press the button or
//! key you want — see [`Settings::controls_activate`]) or removes one.
//! [`crate::ui::settings`] renders it.

use crate::config::{AppConfig, CursorMode, MemoryProfile, ToolbarPosition};
use crate::event::bindings::{self, Action, Store};

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
    /// Rebinding: a list of actions, each showing its gamepad + keyboard bindings,
    /// with add (capture) / remove. Built dynamically, not from [`FIELDS`] — see
    /// [`Settings::controls_rows`].
    Controls,
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
    pub const ALL: [SettingsSection; 7] = [
        SettingsSection::Browser,
        SettingsSection::Display,
        SettingsSection::Input,
        SettingsSection::Controls,
        SettingsSection::Content,
        SettingsSection::Advanced,
        SettingsSection::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsSection::Browser => "Browser",
            SettingsSection::Display => "Display",
            SettingsSection::Input => "Input",
            SettingsSection::Controls => "Controls",
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
/// has no `Field`s — see [`CtrlRow`].
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

/// Every editable config field, in display order (grouped by [`SettingsSection`]).
/// Adding a setting is adding a row here plus an arm in the typed get/set helpers
/// below. `restart = true` marks fields the running app can't apply live. The
/// Controls section is not here — it's built dynamically (see [`Settings::controls_rows`]).
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
    f(S::Display,  "Display",     "Toolbar position",       F::ToolbarPosition,    Kind::Choice(ToolbarPosition::CHOICES), false),
    f(S::Display,  "Display",     "Auto-hide toolbar",      F::ToolbarAutohide,    Kind::Bool, false),

    f(S::Input,  "Input",     "Stick dead zone",        F::Deadzone,           Kind::Float { min: 0.0, max: 0.9, step: 0.05, decimals: 2 }, false),
    f(S::Input,  "Input",     "Cursor speed",           F::CursorSpeed,        Kind::Float { min: 100.0, max: 3000.0, step: 50.0, decimals: 0 }, false),
    f(S::Input,  "Input",     "Scroll speed",           F::ScrollSpeed,        Kind::Float { min: 100.0, max: 5000.0, step: 100.0, decimals: 0 }, false),
    f(S::Input,  "Input",     "Trigger threshold",      F::TriggerThreshold,   Kind::Float { min: 0.1, max: 0.9, step: 0.05, decimals: 2 }, false),
    f(S::Input,  "Input",     "OSK stick threshold",    F::OskNavThreshold,    Kind::Float { min: 0.1, max: 0.9, step: 0.05, decimals: 2 }, false),
    f(S::Input,  "Input",     "OSK repeat delay (ms)",  F::OskNavInitialDelay, Kind::Int { min: 50, max: 1000, step: 50 }, false),
    f(S::Input,  "Input",     "OSK repeat rate (ms)",   F::OskNavRepeat,       Kind::Int { min: 20, max: 500, step: 10 }, false),
    f(S::Input,  "Input",     "Hold gesture (ms)",      F::HoldMs,             Kind::Int { min: 100, max: 2000, step: 50 }, false),
    f(S::Input,  "Input",     "Cursor mode",            F::CursorMode,         Kind::Choice(CursorMode::CHOICES), true),
    f(S::Input,  "Input",     "Hint badges",            F::HintBadges,         Kind::Bool, false),

    f(S::Content,  "History",     "Record history",         F::HistoryEnabled,     Kind::Bool, false),
    f(S::Content,  "History",     "Max entries",            F::HistoryMax,         Kind::Int { min: 0, max: 1000, step: 5 }, false),
    f(S::Content,  "Ad blocker",  "Enabled",                F::AdblockEnabled,     Kind::Bool, true),
    f(S::Content,  "Ad blocker",  "Update every (days)",    F::AdblockUpdateDays,  Kind::Int { min: 0, max: 90, step: 1 }, false),

    f(S::Content, "Data saving", "Block images",         F::BlockImages,        Kind::Bool, false),
    f(S::Content, "Data saving", "Block audio/video",    F::BlockMedia,         Kind::Bool, false),
    f(S::Content, "Data saving", "Block web fonts",      F::BlockFonts,         Kind::Bool, false),

    f(S::Advanced, "Performance", "Memory profile",          F::MemoryProfile,     Kind::Choice(MemoryProfile::CHOICES), true),
    f(S::Advanced, "Performance", "Layout threads (0=auto)", F::LayoutThreads,     Kind::Int { min: 0, max: 8, step: 1 }, true),
    f(S::Advanced, "Performance", "Worker pool max (0=auto)", F::WorkerPoolMax,    Kind::Int { min: 0, max: 16, step: 1 }, true),
    f(S::Advanced, "Downloads",   "Save folder",            F::DownloadDir,        Kind::Text, true),
    f(S::Advanced, "Diagnostics", "Memory overlay",         F::MemoryOverlay,      Kind::Bool, false),
];

/// The bindable actions shown in the Controls section, in display order — every
/// [`Action`] except `None` (removal handles unbinding). `Scroll` is gamepad-only
/// (a keyboard binding for it is rejected on apply).
const CONTROLS_ACTIONS: &[Action] = &[
    Action::Confirm,
    Action::Cancel,
    Action::Osk,
    Action::Reload,
    Action::Prev,
    Action::Next,
    Action::Hints,
    Action::Bookmark,
    Action::Home,
    Action::Reader,
    Action::Menu,
    Action::Settings,
    Action::Quit,
    Action::TabNext,
    Action::TabPrev,
    Action::NewTab,
    Action::ZoomIn,
    Action::ZoomOut,
    Action::ZoomReset,
    Action::NavUp,
    Action::NavDown,
    Action::NavLeft,
    Action::NavRight,
    Action::Scroll,
];

/// One rendered row of the dynamic Controls list (built on demand from the
/// bindings draft by [`Settings::controls_rows`]; not a static [`FIELDS`] row).
#[derive(Clone)]
pub enum CtrlRow {
    /// Action group header (not selectable) — the action's display name.
    Header(&'static str),
    /// An existing binding for an action; activating it removes the binding.
    Binding {
        gesture: String,
        keyboard: bool,
    },
    /// The "add a binding" row for an action; activating it starts capture.
    Add(Action),
    /// Restore the gamepad / keyboard default bindings.
    GamepadReset,
    KeyboardReset,
}

impl CtrlRow {
    /// Header rows are labels only — every other row can be focused / activated.
    fn selectable(&self) -> bool {
        !matches!(self, CtrlRow::Header(_))
    }
}

/// Settings overlay state: visibility, the working drafts, the active section,
/// and the focused row.
pub struct Settings {
    visible: bool,
    /// The config being edited — a clone of the live one taken on [`Self::open`].
    /// Rows mutate this; the app reads it back on close to save and re-apply.
    draft: AppConfig,
    /// The active section (one tab of the bar).
    section: SettingsSection,
    /// Focused row. In a config section it's a [`FIELDS`] index; in the Controls
    /// section it's an index into [`Self::controls_rows`] (the active section
    /// decides which, since focus only moves within it).
    selected: usize,
    /// The bindings being edited (the Controls section), a clone of the on-disk
    /// store taken on [`Self::open`]. Kept independent of `draft` so a config-only
    /// edit never rewrites `bindings.toml` and vice versa.
    bindings_draft: Store,
    /// The bindings as seeded on [`Self::open`], to diff the draft against on close
    /// — so `bindings.toml` is only rewritten when the controls actually changed
    /// (a config-only edit leaves the file, and any hand-written comments, alone).
    bindings_orig: Store,
    /// While `Some`, the overlay is listening for a gesture (gamepad button or key
    /// combo) to add to this action. The event loop routes raw input here instead
    /// of dispatching it (see [`crate::event::handler`] / [`crate::event::gamepad`]).
    capturing: Option<Action>,
}

impl Settings {
    pub fn new() -> Self {
        Self {
            visible: false,
            draft: AppConfig::default(),
            section: SettingsSection::Browser,
            selected: 0,
            bindings_draft: Store::default(),
            bindings_orig: Store::default(),
            capturing: None,
        }
    }

    /// All config field descriptors, in display order (the renderer filters by
    /// section; the Controls section is built separately).
    pub fn fields() -> &'static [Field] {
        FIELDS
    }

    #[inline]
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Open the overlay, seeding both drafts from disk and focusing the first row
    /// of the first section. Each bindings table is filled from its defaults when
    /// empty so the Controls list shows the *effective* (running) bindings; since
    /// the file is only rewritten when the draft actually changes, a fresh/empty
    /// file with comments is left intact.
    pub fn open(&mut self, config: &AppConfig) {
        self.draft = config.clone();
        self.bindings_draft = bindings::load_store();
        if self.bindings_draft.gamepad.is_empty() {
            self.bindings_draft.gamepad = bindings::default_gamepad_bindings();
        }
        if self.bindings_draft.keyboard.is_empty() {
            self.bindings_draft.keyboard = bindings::default_keyboard_bindings();
        }
        self.bindings_orig = self.bindings_draft.clone();
        self.section = SettingsSection::Browser;
        self.selected = 0;
        self.capturing = None;
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
        // Drop any pending capture — otherwise `capturing()` would keep the event
        // loop swallowing input after the overlay is gone.
        self.capturing = None;
    }

    /// The edited config (cloned out by the app on close to save + apply).
    pub fn draft(&self) -> AppConfig {
        self.draft.clone()
    }

    /// The edited bindings store, but only when the controls actually changed —
    /// `None` leaves `bindings.toml` (and its comments) untouched on a config-only
    /// edit. `Some` is cloned out by the app on close to save + reload.
    pub fn changed_bindings(&self) -> Option<Store> {
        (self.bindings_draft != self.bindings_orig).then(|| self.bindings_draft.clone())
    }

    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    #[inline]
    pub fn section(&self) -> SettingsSection {
        self.section
    }

    /// Whether the active section is a config field list (not Controls or About).
    fn is_field_section(&self) -> bool {
        !matches!(
            self.section,
            SettingsSection::Controls | SettingsSection::About
        )
    }

    /// Whether the active section is the dynamic Controls list (driven by
    /// [`Self::controls_rows`] / [`Self::controls_activate`] rather than [`FIELDS`]).
    pub fn is_controls_section(&self) -> bool {
        matches!(self.section, SettingsSection::Controls)
    }

    /// Whether the active section is the read-only [`SettingsSection::About`] page.
    pub fn is_info_section(&self) -> bool {
        matches!(self.section, SettingsSection::About)
    }

    /// Focus a row directly (clicking it). In the Controls section `i` indexes
    /// [`Self::controls_rows`]; otherwise it's a [`FIELDS`] index (and syncs the
    /// active section to it).
    pub fn set_selected(&mut self, i: usize) {
        if self.is_controls_section() {
            self.selected = i;
        } else if let Some(field) = FIELDS.get(i) {
            self.section = field.section;
            self.selected = i;
        }
    }

    /// Jump straight to a section (clicking its tab), focusing its first row.
    pub fn set_section(&mut self, section: SettingsSection) {
        self.section = section;
        self.selected = if section == SettingsSection::Controls {
            self.first_controls_selectable()
        } else {
            FIELDS
                .iter()
                .position(|f| f.section == section)
                .unwrap_or(0)
        };
    }

    /// Switch the active section by `delta` (L1/R1; clamped, no wrap).
    pub fn switch_section(&mut self, delta: i32) {
        let last = SettingsSection::ALL.len() as i32 - 1;
        let i = (self.section.index() as i32 + delta).clamp(0, last) as usize;
        self.set_section(SettingsSection::ALL[i]);
    }

    /// Move the focus by `dy` rows within the active section (clamped, no wrap),
    /// skipping the Controls section's non-selectable headers.
    pub fn move_sel(&mut self, dy: i32) {
        let rows = if self.is_controls_section() {
            Self::ctrl_selectable(&self.controls_rows())
        } else {
            self.section_indices()
        };
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

    /// Whether the focused row holds free text (A opens the OSK on it). Only ever
    /// true in a config section.
    pub fn selected_is_text(&self) -> bool {
        self.is_field_section() && matches!(FIELDS[self.selected].kind, Kind::Text)
    }

    /// Whether row `i` shows ◀▶ step buttons — numbers only (bools/choices toggle
    /// on click instead). Config sections only.
    pub fn is_steppable(&self, i: usize) -> bool {
        matches!(FIELDS[i].kind, Kind::Int { .. } | Kind::Float { .. })
    }

    /// The OSK's edit buffer for the focused row — the draft's own `String` for a
    /// `Text` field, so typing lands straight in the draft. `None` otherwise.
    pub fn selected_text_mut(&mut self) -> Option<&mut String> {
        if !self.is_field_section() {
            return None;
        }
        let c = &mut self.draft;
        match FIELDS[self.selected].id {
            FieldId::HomePage => Some(&mut c.browser.home_page),
            FieldId::SearchPage => Some(&mut c.browser.search_page),
            FieldId::DownloadDir => Some(&mut c.downloads.dir),
            _ => None,
        }
    }

    /// Adjust the focused config field by `dx` (◀ = -1, ▶ = +1): toggle a bool,
    /// cycle a choice, or step a number within its bounds. No-op outside config
    /// sections (Controls edits via A; About is read-only).
    pub fn adjust(&mut self, dx: i32) {
        if !self.is_field_section() {
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

    /// The display string for config row `i`'s current value.
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

    // --- Controls section: a dynamic action list over the bindings draft. ---

    /// Whether the overlay is listening for a binding right now — the event loop
    /// routes raw input to [`Self::apply_capture`] / [`Self::cancel_capture`]
    /// while this holds.
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// The action currently being bound (for the renderer's "listening" hint).
    pub fn capturing_action(&self) -> Option<Action> {
        self.capturing
    }

    /// Build the Controls rows from the bindings draft: per action, a header, a
    /// row per existing binding (gamepad then keyboard), and an "add" row; then
    /// the two reset rows. Rebuilt on demand — `selected` indexes into the result.
    pub fn controls_rows(&self) -> Vec<CtrlRow> {
        let mut rows = Vec::new();
        for &action in CONTROLS_ACTIONS {
            rows.push(CtrlRow::Header(action.display()));
            let name = action.name();
            for gesture in self.bound_gestures(&self.bindings_draft.gamepad, name) {
                rows.push(CtrlRow::Binding {
                    gesture,
                    keyboard: false,
                });
            }
            for gesture in self.bound_gestures(&self.bindings_draft.keyboard, name) {
                rows.push(CtrlRow::Binding {
                    gesture,
                    keyboard: true,
                });
            }
            rows.push(CtrlRow::Add(action));
        }
        rows.push(CtrlRow::GamepadReset);
        rows.push(CtrlRow::KeyboardReset);
        rows
    }

    /// The gestures in `table` bound to action `name`, in key order.
    fn bound_gestures(
        &self,
        table: &std::collections::BTreeMap<String, String>,
        name: &str,
    ) -> Vec<String> {
        table
            .iter()
            .filter(|(_, a)| a.as_str() == name)
            .map(|(g, _)| g.clone())
            .collect()
    }

    /// Indices of the selectable (non-header) rows in `rows`, in order.
    fn ctrl_selectable(rows: &[CtrlRow]) -> Vec<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, r)| r.selectable())
            .map(|(i, _)| i)
            .collect()
    }

    /// The first selectable Controls row (the header is row 0).
    fn first_controls_selectable(&self) -> usize {
        Self::ctrl_selectable(&self.controls_rows())
            .first()
            .copied()
            .unwrap_or(0)
    }

    /// Keep `selected` on a valid selectable row after the list changes (a binding
    /// added or removed, or a reset) — nearest selectable at or before it.
    fn clamp_controls_selection(&mut self) {
        let rows = self.controls_rows();
        let sel = Self::ctrl_selectable(&rows);
        if sel.contains(&self.selected) {
            return;
        }
        self.selected = sel
            .iter()
            .rev()
            .find(|&&i| i <= self.selected)
            .or_else(|| sel.first())
            .copied()
            .unwrap_or(0);
    }

    /// Focus an action's "add" row (after a capture, so repeated adds are easy).
    fn focus_add(&mut self, action: Action) {
        if let Some(i) = self
            .controls_rows()
            .iter()
            .position(|r| matches!(r, CtrlRow::Add(a) if *a == action))
        {
            self.selected = i;
        }
    }

    /// A / Enter / click on the focused Controls row: start capture on an "add"
    /// row, remove a binding row, or restore defaults on a reset row.
    pub fn controls_activate(&mut self) {
        let rows = self.controls_rows();
        match rows.get(self.selected) {
            Some(CtrlRow::Add(action)) => self.capturing = Some(*action),
            Some(CtrlRow::Binding {
                gesture, keyboard, ..
            }) => {
                let (gesture, keyboard) = (gesture.clone(), *keyboard);
                if keyboard {
                    self.bindings_draft.keyboard.remove(&gesture);
                } else {
                    self.bindings_draft.gamepad.remove(&gesture);
                }
                self.clamp_controls_selection();
            }
            Some(CtrlRow::GamepadReset) => {
                self.bindings_draft.gamepad = bindings::default_gamepad_bindings();
                self.clamp_controls_selection();
            }
            Some(CtrlRow::KeyboardReset) => {
                self.bindings_draft.keyboard = bindings::default_keyboard_bindings();
                self.clamp_controls_selection();
            }
            Some(CtrlRow::Header(_)) | None => {}
        }
    }

    /// Bind the captured `gesture` to the listening action and stop listening. The
    /// gesture replaces any other action on that exact gesture (a gesture maps to
    /// one action); a keyboard binding for the gamepad-only `Scroll` is dropped.
    pub fn apply_capture(&mut self, gesture: String, keyboard: bool) {
        let Some(action) = self.capturing.take() else {
            return;
        };
        if keyboard {
            if action == Action::Scroll {
                return;
            }
            self.bindings_draft
                .keyboard
                .insert(gesture, action.name().to_string());
        } else {
            self.bindings_draft
                .gamepad
                .insert(gesture, action.name().to_string());
        }
        self.focus_add(action);
    }

    /// Stop listening without changing anything (Esc / timeout).
    pub fn cancel_capture(&mut self) {
        self.capturing = None;
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
            FieldId::MemoryOverlay => c.debug.memory_overlay,
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
            FieldId::MemoryOverlay => c.debug.memory_overlay = b,
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
