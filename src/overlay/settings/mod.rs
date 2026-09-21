//! The full-screen settings overlay: the config fields that [`crate::config::AppConfig`]
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
/// bar stays narrow; within those the field's `cat` becomes a sub-header.
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

/// The focused row, in the space its section owns — so an About row index can
/// never reach [`fields::FIELDS`] and switch sections by accident.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sel {
    /// A [`fields::FIELDS`] index (the config sections).
    Field(usize),
    /// A flat About-tab row: the update block, then the static links.
    About(usize),
}

/// What one visit to the overlay edits: the config and bindings drafts, the
/// active section and the focused row. Lives exactly as long as the screen is
/// open, so nothing here is resident while it is closed.
struct Draft {
    /// The config being edited — a clone of the live one taken on [`Settings::open`].
    /// Rows mutate this; the app reads it back on close to save and re-apply.
    config: AppConfig,
    /// The active section (one tab of the bar).
    section: SettingsSection,
    /// Focused row, in the space its section owns (see [`Sel`]). The Controls
    /// section keeps its own cursor inside [`Self::controls`], which spans the
    /// reset rows after the editor's own.
    selected: Sel,
    /// The bindings being edited (the Controls section), a clone of the on-disk
    /// store taken on [`Settings::open`]. Kept independent of `config` so a
    /// config-only edit never rewrites `bindings.toml` and vice versa.
    bindings: Store,
    /// The bindings as seeded on [`Settings::open`], to diff the draft against on
    /// close — so `bindings.toml` is only rewritten when the controls actually
    /// changed (a config-only edit leaves the file, and its comments, alone).
    bindings_orig: Store,
    /// The action row awaiting its confirming second press, as a
    /// [`fields::FIELDS`] index. Any move or section change disarms it.
    armed: Option<usize>,
    /// The Controls section's rows and its pending capture (see [`controls`]).
    controls: Controls<Action>,
    /// Why the last binding edit was refused; cleared by the next one.
    controls_note: Option<String>,
}

impl Draft {
    /// Whether the active section is a config field list (not Controls or About).
    fn is_field_section(&self) -> bool {
        !matches!(
            self.section,
            SettingsSection::Controls | SettingsSection::About
        )
    }

    fn is_controls_section(&self) -> bool {
        matches!(self.section, SettingsSection::Controls)
    }

    fn is_info_section(&self) -> bool {
        matches!(self.section, SettingsSection::About)
    }

    /// Jump straight to a section, focusing its first row.
    fn set_section(&mut self, section: SettingsSection) {
        self.section = section;
        self.armed = None;
        if section == SettingsSection::Controls {
            self.controls.focus_first();
            return;
        }
        if section == SettingsSection::About {
            self.selected = Sel::About(0);
            return;
        }
        let first = fields::FIELDS
            .iter()
            .position(|f| f.section == section)
            .unwrap_or(0);
        self.selected = Sel::Field(first);
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
}

/// The About tab's gamepad-focusable rows: the update block, whose `update_rows`
/// the caller computes from the live state, then the static links.
fn about_row_count(update_rows: usize) -> usize {
    update_rows + about_info().links.len()
}

/// The settings overlay. The drafts are the screen's: closing frees them, and
/// reopening seeds fresh ones from disk.
pub struct Settings {
    draft: Option<Box<Draft>>,
}

impl Settings {
    pub fn new() -> Self {
        Self { draft: None }
    }

    /// All config field descriptors, in display order (the renderer filters by
    /// section; the Controls section is built separately).
    pub fn fields() -> &'static [Field] {
        fields::FIELDS
    }

    #[inline]
    pub fn visible(&self) -> bool {
        self.draft.is_some()
    }

    #[inline]
    fn draft(&self) -> Option<&Draft> {
        self.draft.as_deref()
    }

    #[inline]
    fn draft_mut(&mut self) -> Option<&mut Draft> {
        self.draft.as_deref_mut()
    }

    /// Open the overlay, seeding both drafts from disk and focusing the first row
    /// of the first section.
    pub fn open(&mut self, config: &AppConfig) {
        let bindings = bindings::load_store();
        let mut draft = Box::new(Draft {
            config: config.clone(),
            section: SettingsSection::Browser,
            selected: Sel::Field(0),
            bindings_orig: bindings.clone(),
            bindings,
            armed: None,
            controls: Controls::new(GROUPS, SURFACES, RESET_ROWS),
            controls_note: None,
        });
        draft.show_controls();
        self.draft = Some(draft);
    }

    /// Close it, handing back the edited config and the bindings to write, the
    /// latter only when the controls changed — a config-only edit leaves
    /// `bindings.toml` and its comments alone. `None` when it was not open.
    pub fn close(&mut self) -> Option<(AppConfig, Option<Store>)> {
        let draft = *self.draft.take()?;
        let bindings = (draft.bindings != draft.bindings_orig).then_some(draft.bindings);
        Some((draft.config, bindings))
    }

    /// The draft's pending `[update]` edits while the overlay is open, so an About-tab
    /// check sees a channel switched in this same visit (the config is only handed
    /// over on close). `None` once closed — `apply_config` has adopted it by then.
    #[inline]
    pub fn pending_update(&self) -> Option<&crate::config::UpdateConfig> {
        self.draft().map(|draft| &draft.config.update)
    }

    /// The focused row, flattened for the renderer: the Controls cursor, a
    /// [`fields::FIELDS`] index, or an About row — the section says which.
    #[inline]
    pub fn selected(&self) -> usize {
        let Some(draft) = self.draft() else {
            return 0;
        };
        if draft.is_controls_section() {
            return draft.controls.cursor();
        }
        match draft.selected {
            Sel::Field(i) | Sel::About(i) => i,
        }
    }

    /// The active section, or the first one while the overlay is closed.
    #[inline]
    pub fn section(&self) -> SettingsSection {
        self.draft()
            .map_or(SettingsSection::Browser, |draft| draft.section)
    }

    /// Whether the active section is the dynamic Controls list (driven by
    /// [`Self::controls_rows`] / [`Self::controls_activate`] rather than [`fields::FIELDS`]).
    pub fn is_controls_section(&self) -> bool {
        self.draft().is_some_and(Draft::is_controls_section)
    }

    /// Whether the active section is the read-only [`SettingsSection::About`] page.
    pub fn is_info_section(&self) -> bool {
        self.draft().is_some_and(Draft::is_info_section)
    }

    /// Focus a row directly. In the Controls section `i` indexes
    /// [`Self::controls_rows`]; otherwise it's a [`fields::FIELDS`] index (and syncs
    /// the active section to it).
    pub fn set_selected(&mut self, i: usize) {
        let Some(draft) = self.draft_mut() else {
            return;
        };
        if draft.armed != Some(i) {
            draft.armed = None;
        }
        if draft.is_controls_section() {
            draft.controls.set_cursor(i);
        } else if let Some(field) = fields::FIELDS.get(i) {
            draft.section = field.section;
            draft.selected = Sel::Field(i);
        }
    }

    /// Jump straight to a section, focusing its first row.
    pub fn set_section(&mut self, section: SettingsSection) {
        let Some(draft) = self.draft_mut() else {
            return;
        };
        draft.set_section(section);
    }

    /// Switch the active section by `delta` (clamped, no wrap).
    pub fn switch_section(&mut self, delta: i32) {
        let Some(draft) = self.draft_mut() else {
            return;
        };
        let i = crate::list::step(draft.section.index(), delta, SettingsSection::ALL.len());
        draft.set_section(SettingsSection::ALL[i]);
    }

    /// Move the focus by `dy` rows within the active section (clamped, no wrap),
    /// skipping the Controls section's non-selectable headers. `update_rows` is the
    /// About tab's live update-block row count (ignored in other sections).
    pub fn move_sel(&mut self, dy: i32, update_rows: usize) {
        let Some(draft) = self.draft_mut() else {
            return;
        };
        draft.armed = None;
        if draft.is_info_section() {
            // About: a flat list (update rows, then links), all selectable.
            let at = match draft.selected {
                Sel::About(i) => i,
                Sel::Field(_) => 0,
            };
            let to = crate::list::step(at, dy, about_row_count(update_rows));
            draft.selected = Sel::About(to);
            return;
        }
        if draft.is_controls_section() {
            draft.controls.move_cursor(dy);
            return;
        }
        let Sel::Field(cur) = draft.selected else {
            return;
        };
        let rows = draft.section_indices();
        let Some(pos) = rows.iter().position(|&g| g == cur) else {
            return;
        };
        let np = crate::list::step(pos, dy, rows.len());
        draft.selected = Sel::Field(rows[np]);
    }

    /// Whether the focused row holds free text (A opens the OSK on it). Only ever
    /// true in a config section.
    pub fn selected_is_text(&self) -> bool {
        let Some(draft) = self.draft() else {
            return false;
        };
        let Sel::Field(i) = draft.selected else {
            return false;
        };
        draft.is_field_section() && matches!(fields::FIELDS[i].kind, Kind::Text { .. })
    }

    /// Whether row `i` shows step buttons — numbers only (bools/choices toggle
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
        let draft = self.draft_mut()?;
        let Sel::Field(i) = draft.selected else {
            return None;
        };
        if !draft.is_field_section() {
            return None;
        }
        match &fields::FIELDS[i].kind {
            Kind::Text { get_mut, .. } => Some(get_mut(&mut draft.config)),
            _ => None,
        }
    }

    /// A on the focused row: `Some` when it is an action row pressed a second
    /// time — the first press only arms it (and [`Self::adjust`], which the
    /// caller falls through to, is a no-op on this kind).
    pub fn confirm_action(&mut self) -> Option<Task> {
        let draft = self.draft_mut()?;
        let Sel::Field(i) = draft.selected else {
            return None;
        };
        if !draft.is_field_section() {
            return None;
        }
        let Kind::Action { task } = &fields::FIELDS[i].kind else {
            return None;
        };
        if draft.armed.replace(i) == Some(i) {
            draft.armed = None;
            return Some(*task);
        }
        None
    }

    /// Config and bindings back to their defaults ([`Task::RestoreDefaults`]; the
    /// pins are the app's). Draft edits like any other, so the close saves them.
    pub fn restore_defaults(&mut self) {
        let Some(draft) = self.draft_mut() else {
            return;
        };
        draft.config = AppConfig::default();
        draft.bindings = bindings::default_store();
        draft.show_controls();
    }

    /// Adjust the focused config field by `dx` (-1 left, +1 right): toggle a bool,
    /// cycle a choice, or step a number within its bounds. No-op outside config
    /// sections (Controls edits on activate; About is read-only).
    pub fn adjust(&mut self, dx: i32) {
        let Some(draft) = self.draft_mut() else {
            return;
        };
        let Sel::Field(i) = draft.selected else {
            return;
        };
        if !draft.is_field_section() {
            return;
        }
        let config = &mut draft.config;
        match &fields::FIELDS[i].kind {
            Kind::Text { .. } | Kind::Action { .. } => {}
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

    /// The display string for config row `i`'s current value; empty while the
    /// overlay is closed, which is when no row is drawn.
    pub fn value_str(&self, i: usize) -> String {
        let Some(draft) = self.draft() else {
            return String::new();
        };
        let config = &draft.config;
        match &fields::FIELDS[i].kind {
            Kind::Action { task } => if draft.armed == Some(i) {
                "press again to confirm"
            } else {
                task.verb()
            }
            .to_string(),
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
            Kind::Float { decimals, get, .. } => {
                format!("{:.*}", decimals, get(config))
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
        let (config, _) = settings.close().expect("it was open");
        assert_eq!(config.update.channel, Channel::Beta);
        assert!(settings.pending_update().is_none());
        // Closing is what frees the drafts, so a second close has nothing to hand back.
        assert!(settings.close().is_none());
    }

    /// The [`fields::FIELDS`] index of `task`'s action row.
    fn row_of(task: Task) -> usize {
        let runs = |f: &Field| matches!(f.kind, Kind::Action { task: t } if t == task);
        fields::FIELDS
            .iter()
            .position(runs)
            .unwrap_or_else(|| panic!("FIELDS lists an action row for {task:?}"))
    }

    fn clear_row() -> usize {
        row_of(Task::ClearData)
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

    /// The restore row hands back its task on the second press, and applying it
    /// puts the config and the bindings back in the drafts the close saves.
    #[test]
    fn restore_defaults_resets_the_drafts() {
        let mut cfg = AppConfig::default();
        cfg.browser.home_page = "https://example.org/".to_string();
        cfg.display.scale = 1.45;
        cfg.input.hint_badges = !cfg.input.hint_badges;

        let row = row_of(Task::RestoreDefaults);
        let mut settings = Settings::new();
        settings.open(&cfg);
        settings
            .draft_mut()
            .expect("just opened")
            .bindings
            .gamepad
            .remove("a");
        settings.set_selected(row);

        assert_eq!(settings.value_str(row), "Restore");
        assert_eq!(settings.confirm_action(), None);
        assert_eq!(settings.confirm_action(), Some(Task::RestoreDefaults));
        settings.restore_defaults();

        let defaults = AppConfig::default();
        let draft = settings.draft().expect("still open");
        assert_eq!(draft.config.browser.home_page, defaults.browser.home_page);
        assert_eq!(draft.config.display.scale, defaults.display.scale);
        assert_eq!(draft.config.input.hint_badges, defaults.input.hint_badges);
        assert_eq!(draft.bindings, bindings::default_store());
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
