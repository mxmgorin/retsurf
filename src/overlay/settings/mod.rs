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
pub use controls::RESET_ROWS;
pub use fields::{Field, Kind, Task};

use crate::config::AppConfig;
use crate::event::bindings::{self, Action, GROUPS, SURFACES};
use inputbind::editor::Controls;
use inputbind::Store;

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
    /// Focused row in a config section, a [`fields::FIELDS`] index. The Controls
    /// section keeps its own inside [`Self::controls`], which spans the reset
    /// rows after the editor's own.
    selected: usize,
    /// The bindings being edited (the Controls section), a clone of the on-disk
    /// store taken on [`Self::open`]. Kept independent of `draft` so a config-only
    /// edit never rewrites `bindings.toml` and vice versa.
    bindings_draft: Store,
    /// The bindings as seeded on [`Self::open`], to diff the draft against on close
    /// — so `bindings.toml` is only rewritten when the controls actually changed
    /// (a config-only edit leaves the file, and any hand-written comments, alone).
    bindings_orig: Store,
    /// The action row awaiting its confirming second press, as a
    /// [`fields::FIELDS`] index. Any move or section change disarms it.
    armed: Option<usize>,
    /// The Controls section's rows and its pending capture (see [`controls`]).
    controls: Controls<Action>,
    /// Why the last binding edit was refused; cleared by the next one.
    controls_note: Option<String>,
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
            armed: None,
            controls: Controls::new(GROUPS, SURFACES, RESET_ROWS),
            controls_note: None,
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
    /// of the first section.
    pub fn open(&mut self, config: &AppConfig) {
        self.draft = config.clone();
        self.bindings_draft = bindings::load_store();
        self.bindings_orig = self.bindings_draft.clone();
        self.show_controls();
        self.section = SettingsSection::Browser;
        self.selected = 0;
        self.armed = None;
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.armed = None;
        // Drop any pending capture — otherwise `capturing()` would keep the event
        // loop swallowing input after the overlay is gone.
        self.controls.close();
    }

    /// The edited config (cloned out by the app on close to save + apply).
    pub fn draft(&self) -> AppConfig {
        self.draft.clone()
    }

    /// The draft's pending `[update]` edits while the overlay is open, so an About-tab
    /// check sees a channel switched in this same visit (the full draft is only handed
    /// over on close). `None` once closed — `apply_config` has adopted it by then.
    #[inline]
    pub fn pending_update(&self) -> Option<&crate::config::UpdateConfig> {
        self.visible.then_some(&self.draft.update)
    }

    /// The edited bindings store, but only when the controls actually changed —
    /// `None` leaves `bindings.toml` (and its comments) untouched on a config-only
    /// edit. `Some` is cloned out by the app on close to save + reload.
    pub fn changed_bindings(&self) -> Option<Store> {
        (self.bindings_draft != self.bindings_orig).then(|| self.bindings_draft.clone())
    }

    /// The focused row. In the Controls section the editor owns the cursor, so
    /// there is only ever one; elsewhere it is a [`fields::FIELDS`] index.
    #[inline]
    pub fn selected(&self) -> usize {
        if self.is_controls_section() {
            self.controls_cursor()
        } else {
            self.selected
        }
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

    /// Number of gamepad-focusable rows on the About tab: the update block's
    /// `update_rows` (action row, plus a release-notes link when an update is
    /// available — computed from the live update state by the caller) then the
    /// static links. Drives [`Self::move_sel`] and the renderer's highlight.
    pub fn about_row_count(&self, update_rows: usize) -> usize {
        update_rows + about_info().links.len()
    }

    /// Focus a row directly (clicking it). In the Controls section `i` indexes
    /// [`Self::controls_rows`]; otherwise it's a [`fields::FIELDS`] index (and syncs
    /// the active section to it).
    pub fn set_selected(&mut self, i: usize) {
        if self.armed != Some(i) {
            self.armed = None;
        }
        if self.is_controls_section() {
            self.controls_set_cursor(i);
        } else if let Some(field) = fields::FIELDS.get(i) {
            self.section = field.section;
            self.selected = i;
        }
    }

    /// Jump straight to a section (clicking its tab), focusing its first row.
    pub fn set_section(&mut self, section: SettingsSection) {
        self.section = section;
        self.armed = None;
        if section == SettingsSection::Controls {
            self.focus_first_control();
            return;
        }
        self.selected = fields::FIELDS
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

    /// Move the focus by `dy` rows within the active section (clamped, no wrap),
    /// skipping the Controls section's non-selectable headers. `update_rows` is the
    /// About tab's live update-block row count (ignored in other sections).
    pub fn move_sel(&mut self, dy: i32, update_rows: usize) {
        self.armed = None;
        if self.is_info_section() {
            // About: a flat list (update rows, then links), all selectable.
            let last = self.about_row_count(update_rows) as i32 - 1;
            self.selected = (self.selected as i32 + dy).clamp(0, last.max(0)) as usize;
            return;
        }
        if self.is_controls_section() {
            self.controls_move(dy);
            return;
        }
        let rows = self.section_indices();
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
        self.is_field_section() && matches!(fields::FIELDS[self.selected].kind, Kind::Text { .. })
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
        match &fields::FIELDS[self.selected].kind {
            Kind::Text { get_mut, .. } => Some(get_mut(&mut self.draft)),
            _ => None,
        }
    }

    /// A on the focused row: `Some` when it is an action row pressed a second
    /// time — the first press only arms it (and [`Self::adjust`], which the
    /// caller falls through to, is a no-op on this kind).
    pub fn confirm_action(&mut self) -> Option<Task> {
        if !self.is_field_section() {
            return None;
        }
        let Kind::Action { task } = &fields::FIELDS[self.selected].kind else {
            return None;
        };
        if self.armed.replace(self.selected) == Some(self.selected) {
            self.armed = None;
            return Some(*task);
        }
        None
    }

    /// Adjust the focused config field by `dx` (◀ = -1, ▶ = +1): toggle a bool,
    /// cycle a choice, or step a number within its bounds. No-op outside config
    /// sections (Controls edits via A; About is read-only).
    pub fn adjust(&mut self, dx: i32) {
        if !self.is_field_section() {
            return;
        }
        match &fields::FIELDS[self.selected].kind {
            Kind::Text { .. } | Kind::Action { .. } => {}
            Kind::Bool { get, set } => {
                let v = !get(&self.draft);
                set(&mut self.draft, v);
            }
            Kind::Choice { opts, get, set } => {
                let cur = get(&self.draft);
                let n = opts.len() as i32;
                let idx = opts.iter().position(|(_, v)| *v == cur).unwrap_or(0) as i32;
                let next = (idx + dx).rem_euclid(n) as usize;
                set(&mut self.draft, opts[next].1);
            }
            Kind::Int {
                min,
                max,
                step,
                get,
                set,
                ..
            } => {
                let v = (get(&self.draft) + dx as i64 * step).clamp(*min, *max);
                set(&mut self.draft, v);
            }
            Kind::Float {
                min,
                max,
                step,
                get,
                set,
                ..
            } => {
                let v = (get(&self.draft) + dx as f64 * step).clamp(*min, *max);
                set(&mut self.draft, v);
            }
        }
    }

    /// The display string for config row `i`'s current value.
    pub fn value_str(&self, i: usize) -> String {
        match &fields::FIELDS[i].kind {
            Kind::Action { .. } => if self.armed == Some(i) {
                "press again to confirm"
            } else {
                "Clear"
            }
            .to_string(),
            Kind::Bool { get, .. } => if get(&self.draft) { "On" } else { "Off" }.to_string(),
            Kind::Text { get, .. } => {
                let t = get(&self.draft);
                if t.is_empty() {
                    "(default)".to_string()
                } else {
                    t
                }
            }
            Kind::Choice { opts, get, .. } => {
                let cur = get(&self.draft);
                opts.iter()
                    .find(|(_, v)| *v == cur)
                    .map(|(label, _)| label.to_string())
                    .unwrap_or(cur)
            }
            Kind::Int { zero, get, .. } => {
                let v = get(&self.draft);
                match zero {
                    Some(label) if v == 0 => label.to_string(),
                    _ => format!("{v}"),
                }
            }
            Kind::Float { decimals, get, .. } => {
                format!("{:.*}", decimals, get(&self.draft))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Channel;

    /// An About-tab check runs while the overlay is open, so an edit from the same
    /// visit must be readable before close — and gone after it.
    #[test]
    fn pending_update_follows_the_draft() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.update.channel, Channel::Release);
        let row = fields::FIELDS
            .iter()
            .position(|f| f.label == "Update channel")
            .expect("FIELDS lists the update channel row");

        let mut settings = Settings::new();
        settings.open(&cfg);
        settings.set_selected(row);
        settings.adjust(1);

        assert_eq!(
            settings.pending_update().map(|u| u.channel),
            Some(Channel::Beta)
        );
        settings.close();
        assert!(settings.pending_update().is_none());
    }

    /// The [`fields::FIELDS`] index of the row that clears browsing data.
    fn clear_row() -> usize {
        let clears = |f: &Field| matches!(f.kind, Kind::Action { task } if task == Task::ClearData);
        fields::FIELDS
            .iter()
            .position(clears)
            .expect("FIELDS lists the clear-data row")
    }

    /// One press must never wipe anything: it only arms the row.
    #[test]
    fn an_action_runs_on_the_second_press() {
        let mut settings = Settings::new();
        settings.open(&AppConfig::default());
        settings.set_selected(clear_row());

        assert_eq!(settings.confirm_action(), None);
        assert_eq!(settings.value_str(clear_row()), "press again to confirm");
        assert_eq!(settings.confirm_action(), Some(Task::ClearData));
        // Run, so the row is disarmed again.
        assert_eq!(settings.confirm_action(), None);
    }

    /// Moving off the armed row cancels it — otherwise a stray A elsewhere in
    /// the section would come back to wipe.
    #[test]
    fn moving_off_an_armed_action_disarms_it() {
        let mut settings = Settings::new();
        settings.open(&AppConfig::default());
        settings.set_selected(clear_row());
        settings.confirm_action();

        settings.move_sel(-1, 0);
        settings.set_selected(clear_row());
        assert_eq!(settings.confirm_action(), None);
    }
}
