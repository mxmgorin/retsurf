//! The Controls section: [`inputbind::editor::Controls`] over the bindings draft,
//! plus the two rows restoring a table's defaults, which the editor leaves to the
//! host. An edit that would leave the pad unable to operate the app is refused
//! with a note — see [`bindings::REQUIRED`].

use super::{Draft, Settings};
use crate::event::bindings::{self, Action, REQUIRED};
use inputbind::editor::{validate, Refusal, Row, Source};
use inputbind::Action as _;

/// Restore the gamepad / keyboard defaults.
pub const RESET_ROWS: usize = 2;

impl Settings {
    /// While this holds, the event loop routes raw input to [`Self::apply_capture`].
    pub fn capturing(&self) -> bool {
        self.capturing_action().is_some()
    }

    /// The action being bound, for the renderer's "listening" hint.
    pub fn capturing_action(&self) -> Option<Action> {
        self.draft()?.controls.capturing()
    }

    /// The editor's rows. The reset rows follow at `rows().len()..`.
    pub fn controls_rows(&self) -> &[Row<Action>] {
        self.draft().map_or(&[], |draft| draft.controls.rows())
    }

    /// Why the last edit was refused, shown under the section hint.
    pub fn controls_note(&self) -> Option<&str> {
        self.draft()?.controls_note.as_deref()
    }

    /// A / Enter / click: open a command, unbind a gesture, capture, or restore.
    pub fn controls_activate(&mut self) {
        if let Some(draft) = self.draft_mut() {
            draft.activate();
        }
    }

    /// Replaces whatever else held that gesture: a gesture names one action.
    pub fn apply_capture(&mut self, gesture: String, keyboard: bool) {
        if let Some(draft) = self.draft_mut() {
            draft.apply_capture(gesture, keyboard);
        }
    }

    /// Stop listening without changing anything (Esc / timeout).
    pub fn cancel_capture(&mut self) {
        if let Some(draft) = self.draft_mut() {
            draft.controls.stop_capture();
        }
    }
}

impl Draft {
    /// After every edit, never per frame.
    fn rebuild_controls(&mut self) {
        self.controls.rebuild(&self.bindings);
    }

    pub(super) fn show_controls(&mut self) {
        self.controls.show(&self.bindings);
        self.controls_note = None;
    }

    /// Focus an action's Add row, so repeated adds need no cursor work.
    fn focus_add(&mut self, action: Action) {
        if let Some(i) = self
            .controls
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Add(a) if *a == action))
        {
            self.controls.set_cursor(i);
        }
    }

    fn activate(&mut self) {
        self.controls_note = None;
        if let Some(reset) = self.controls.trailing_cursor() {
            match reset {
                0 => self.bindings.gamepad = bindings::default_store().gamepad,
                1 => self.bindings.keyboard = bindings::default_store().keyboard,
                _ => return,
            }
            self.rebuild_controls();
            return;
        }
        let Some(row) = self.controls.selected() else {
            return;
        };
        match row {
            Row::Command { action, .. } => {
                let action = *action;
                self.controls.toggle_command(action, &self.bindings);
            }
            Row::Add(action) => self.controls.start_capture(*action),
            Row::Gesture { text, source } => {
                let (text, source) = (text.clone(), *source);
                self.unbind(&text, source);
            }
            // A `none` override: dropping it lets the base binding through again.
            Row::Suppressed { text, surface } => {
                let (text, surface) = (text.clone(), *surface);
                if let Some(table) = self.bindings.surface.get_mut(surface) {
                    table.remove(&text);
                }
                self.rebuild_controls();
            }
            Row::Group(_) => {}
        }
    }

    /// Drop a gesture, unless the pad would lose something it cannot work without.
    fn unbind(&mut self, text: &str, source: Source) {
        match source {
            Source::Gamepad => {
                if let Err(refusal) =
                    validate::<Action>(&self.bindings.gamepad, text, None, REQUIRED)
                {
                    self.controls_note = Some(explain(refusal));
                    return;
                }
                self.bindings.gamepad.remove(text);
            }
            Source::Keyboard => _ = self.bindings.keyboard.remove(text),
            Source::Surface(name) => {
                if let Some(table) = self.bindings.surface.get_mut(name) {
                    table.remove(text);
                }
            }
        }
        self.rebuild_controls();
    }

    fn apply_capture(&mut self, gesture: String, keyboard: bool) {
        let Some(action) = self.controls.capturing() else {
            return;
        };
        self.controls.stop_capture();
        self.controls_note = None;
        if let Some(refused) = self.refuse(&gesture, action, keyboard) {
            self.controls_note = Some(refused);
            return;
        }
        let table = if keyboard {
            &mut self.bindings.keyboard
        } else {
            &mut self.bindings.gamepad
        };
        table.insert(gesture, action.name().to_string());
        self.rebuild_controls();
        self.focus_add(action);
    }

    /// Why this binding cannot be made, if it cannot.
    fn refuse(&self, gesture: &str, action: Action, keyboard: bool) -> Option<String> {
        if keyboard {
            // `scroll` latches inside the pad, so a key bound to it would do nothing.
            return (action == Action::Scroll)
                .then(|| format!("{} is gamepad-only", action.display()));
        }
        validate(&self.bindings.gamepad, gesture, Some(action), REQUIRED)
            .err()
            .map(explain)
    }
}

/// Only the host knows the table belongs to the gamepad.
fn explain(refusal: Refusal) -> String {
    match refusal {
        Refusal::PressEdge(pad) => format!("{} must stay a tap", pad.name().to_uppercase()),
        Refusal::Requirement(label) => format!("{label} needs a gesture on the gamepad"),
    }
}
