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
//!
//! The pieces live in submodules: [`fields`] (the static config-field table and
//! its typed get/set), [`controls`] (the dynamic rebinding list), and [`about`]
//! (the read-only About tab).

mod about;
mod controls;
mod fields;

pub use about::about_info;
pub use controls::CtrlRow;
pub use fields::{Field, Kind};

use crate::config::AppConfig;
use crate::event::bindings::{self, Action, Store};
use fields::FieldId;

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
    /// with add (capture) / remove. Built dynamically, not from [`fields::FIELDS`] —
    /// see [`Settings::controls_rows`].
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

/// Settings overlay state: visibility, the working drafts, the active section,
/// and the focused row.
pub struct Settings {
    visible: bool,
    /// The config being edited — a clone of the live one taken on [`Self::open`].
    /// Rows mutate this; the app reads it back on close to save and re-apply.
    draft: AppConfig,
    /// The active section (one tab of the bar).
    section: SettingsSection,
    /// Focused row. In a config section it's a [`fields::FIELDS`] index; in the
    /// Controls section it's an index into [`Self::controls_rows`] (the active
    /// section decides which, since focus only moves within it).
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
        fields::FIELDS
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
    /// [`Self::controls_rows`] / [`Self::controls_activate`] rather than [`fields::FIELDS`]).
    pub fn is_controls_section(&self) -> bool {
        matches!(self.section, SettingsSection::Controls)
    }

    /// Whether the active section is the read-only [`SettingsSection::About`] page.
    pub fn is_info_section(&self) -> bool {
        matches!(self.section, SettingsSection::About)
    }

    /// Number of gamepad-focusable rows on the About tab: the update action (row 0)
    /// then the links. Drives [`Self::move_sel`] and the renderer's highlight.
    pub fn about_row_count(&self) -> usize {
        1 + about_info().links.len()
    }

    /// Focus a row directly (clicking it). In the Controls section `i` indexes
    /// [`Self::controls_rows`]; otherwise it's a [`fields::FIELDS`] index (and syncs
    /// the active section to it).
    pub fn set_selected(&mut self, i: usize) {
        if self.is_controls_section() {
            self.selected = i;
        } else if let Some(field) = fields::FIELDS.get(i) {
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
            fields::FIELDS
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
        if self.is_info_section() {
            // About: a flat list (update action, then links), all selectable.
            let last = self.about_row_count() as i32 - 1;
            self.selected = (self.selected as i32 + dy).clamp(0, last.max(0)) as usize;
            return;
        }
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

    /// Global [`fields::FIELDS`] indices belonging to the active section, in order.
    fn section_indices(&self) -> Vec<usize> {
        fields::FIELDS
            .iter()
            .enumerate()
            .filter(|(_, f)| f.section == self.section)
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether the focused row holds free text (A opens the OSK on it). Only ever
    /// true in a config section.
    pub fn selected_is_text(&self) -> bool {
        self.is_field_section() && matches!(fields::FIELDS[self.selected].kind, Kind::Text)
    }

    /// Whether row `i` shows ◀▶ step buttons — numbers only (bools/choices toggle
    /// on click instead). Config sections only.
    pub fn is_steppable(&self, i: usize) -> bool {
        matches!(
            fields::FIELDS[i].kind,
            Kind::Int { .. } | Kind::Float { .. }
        )
    }

    /// The OSK's edit buffer for the focused row — the draft's own `String` for a
    /// `Text` field, so typing lands straight in the draft. `None` otherwise.
    pub fn selected_text_mut(&mut self) -> Option<&mut String> {
        if !self.is_field_section() {
            return None;
        }
        let c = &mut self.draft;
        match fields::FIELDS[self.selected].id {
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
        let id = fields::FIELDS[i].id;
        match &fields::FIELDS[i].kind {
            Kind::Text => {}
            Kind::Bool => {
                let v = fields::get_bool(&self.draft, id);
                fields::set_bool(&mut self.draft, id, !v);
            }
            Kind::Choice(opts) => {
                let cur = fields::get_choice(&self.draft, id);
                let n = opts.len() as i32;
                let idx = opts.iter().position(|(_, v)| *v == cur).unwrap_or(0) as i32;
                let next = (idx + dx).rem_euclid(n) as usize;
                fields::set_choice(&mut self.draft, id, opts[next].1);
            }
            Kind::Int { min, max, step } => {
                let v = (fields::get_num(&self.draft, id) + dx as f64 * *step as f64)
                    .clamp(*min as f64, *max as f64);
                fields::set_num(&mut self.draft, id, v.round());
            }
            Kind::Float { min, max, step, .. } => {
                let v = (fields::get_num(&self.draft, id) + dx as f64 * step).clamp(*min, *max);
                fields::set_num(&mut self.draft, id, v);
            }
        }
    }

    /// The display string for config row `i`'s current value.
    pub fn value_str(&self, i: usize) -> String {
        let id = fields::FIELDS[i].id;
        match &fields::FIELDS[i].kind {
            Kind::Bool => if fields::get_bool(&self.draft, id) {
                "On"
            } else {
                "Off"
            }
            .to_string(),
            Kind::Text => {
                let t = fields::get_text(&self.draft, id);
                if t.is_empty() {
                    "(default)".to_string()
                } else {
                    t.to_string()
                }
            }
            Kind::Choice(opts) => {
                let cur = fields::get_choice(&self.draft, id);
                opts.iter()
                    .find(|(_, v)| *v == cur)
                    .map(|(label, _)| label.to_string())
                    .unwrap_or_else(|| cur.to_string())
            }
            Kind::Int { .. } => format!("{}", fields::get_num(&self.draft, id) as i64),
            Kind::Float { decimals, .. } => {
                format!("{:.*}", decimals, fields::get_num(&self.draft, id))
            }
        }
    }
}
