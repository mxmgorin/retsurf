//! The Controls section of the settings overlay: a dynamic action list over the
//! bindings draft (not a static [`super::Field`] table). Each [`Action`] shows
//! its gamepad + keyboard bindings with add (capture) / remove rows; activating
//! "add" starts listening for a gesture, which [`Settings::apply_capture`] binds.
//! See [`super`] for the rest of the overlay state.

use super::Settings;
use crate::event::bindings::{self, Action};

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
/// bindings draft by [`Settings::controls_rows`]; not a static [`super::Field`] row).
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

impl Settings {
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
    pub(super) fn ctrl_selectable(rows: &[CtrlRow]) -> Vec<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, r)| r.selectable())
            .map(|(i, _)| i)
            .collect()
    }

    /// The first selectable Controls row (the header is row 0).
    pub(super) fn first_controls_selectable(&self) -> usize {
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
}
