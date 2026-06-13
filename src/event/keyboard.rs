//! Translates raw keyboard input into [`AppCommand`]s — the keyboard
//! counterpart of [`crate::event::gamepad`]. It owns the `[keyboard]` table of
//! `bindings.toml` and applies the firing rules: `nav_*` steps need an open
//! overlay (and auto-repeat while held), plain shortcuts (no Ctrl/Alt) are
//! muted while anything editable has focus, and the menu / hint overlays get
//! their fixed keys first. Whatever isn't consumed is forwarded to the page as
//! a Servo keyboard event.

use crate::app::{AppCommand, InputCommand, MenuAction};
use crate::browser::AppBrowser;
use crate::event::bindings::{Action, KeyBindings};
use crate::ui::{AppUi, Focus};
use sdl2::keyboard::{Keycode, Mod, Scancode};

/// One key edge as SDL reports it, bundled for resolution.
pub struct KeyEvent {
    pub kc: Keycode,
    pub sc: Scancode,
    pub keymod: Mod,
    pub repeat: bool,
    pub pressed: bool,
}

pub struct Keyboard {
    /// Keyboard shortcuts from `bindings.toml` (`[keyboard]`).
    bindings: KeyBindings,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            bindings: KeyBindings::load(),
        }
    }

    /// Resolve one key edge: overlay fixed keys first, then the shortcut
    /// table, and finally fall through to the page.
    pub fn on_key(
        &self,
        key: &KeyEvent,
        ui: &AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // A modal page prompt (select picker / JS dialog) captures the keyboard
        // first: Enter activates, Esc dismisses, the `nav_*` bindings move the
        // focus, and everything else is muted so a shortcut can't fire under
        // the modal (typing goes to its text field through egui). The on-screen
        // keyboard stays above it — that's how a gamepad types into `prompt()`.
        if ui.focus() == Focus::Prompt {
            if key.pressed {
                match key.kc {
                    Keycode::Return | Keycode::KpEnter => {
                        commands.push(AppCommand::Input(InputCommand::Confirm(true)))
                    }
                    Keycode::Escape => commands.push(AppCommand::Input(InputCommand::Cancel)),
                    _ => {
                        if let Some(action) = self.lookup(key, true, true) {
                            if action.is_nav() {
                                action.push_tap(commands);
                            }
                        }
                    }
                }
            }
            return;
        }

        // While the menu is open it captures the keyboard wholesale — both
        // edges, so no stray release reaches the page either.
        if ui.menu_visible() {
            if key.pressed {
                self.on_menu_key(key, commands);
            }
            return;
        }

        if key.pressed {
            self.on_key_down(key, ui, browser, commands);
        } else if ui.hints_visible() && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
            // Hint mode times Enter as a tap-vs-hold gesture, so its release edge
            // decides (click vs open-in-new-tab) in the router rather than going
            // to the page like other key-ups.
            commands.push(AppCommand::Input(InputCommand::Confirm(false)));
        } else {
            browser.handle_input(servo::InputEvent::Keyboard(into_servo(key)));
        }
    }

    /// The menu owns the keyboard: Esc closes, Enter opens, Delete removes;
    /// navigation and shortcuts go through the bindings (arrows are the
    /// default `nav_*` gestures).
    fn on_menu_key(&self, key: &KeyEvent, commands: &mut Vec<AppCommand>) {
        // The menu overlay covers everything, so nothing editable can hold
        // focus — `typing` is moot here.
        if let Some(action) = self.lookup(key, true, false) {
            action.push_tap(commands);
            return;
        }
        match key.kc {
            Keycode::Escape => commands.push(AppCommand::Menu(MenuAction::Close)),
            Keycode::Return | Keycode::KpEnter => {
                commands.push(AppCommand::Menu(MenuAction::OpenSelected))
            }
            Keycode::Delete | Keycode::Backspace => {
                commands.push(AppCommand::Menu(MenuAction::RemoveSelected))
            }
            // P pins / unpins the selected Bookmarks or History entry to the speed
            // dial (Y's role) — a no-op in the other sections (handled in the router).
            Keycode::P => commands.push(AppCommand::Input(InputCommand::Hints)),
            _ => {}
        }
    }

    fn on_key_down(
        &self,
        key: &KeyEvent,
        ui: &AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // Hint mode's fixed keys (its navigation comes from the `nav_*`
        // bindings below). Enter is a tap-vs-hold gesture timed in the router,
        // so only its first edge counts — drop autorepeat, and let the release
        // edge (handled in `on_key`) close the gesture.
        if ui.hints_visible() {
            match key.kc {
                Keycode::Return | Keycode::KpEnter => {
                    if !key.repeat {
                        commands.push(AppCommand::Input(InputCommand::Confirm(true)));
                    }
                    return;
                }
                Keycode::Escape => {
                    commands.push(AppCommand::Input(InputCommand::Cancel));
                    return;
                }
                _ => {}
            }
        }

        // The start page: arrows move the selection and Enter activates — the
        // same intents the gamepad routes. While its search field holds keyboard
        // focus, typing/caret/Enter belong to the text editor; only Down leaves
        // the field for the grid below.
        if ui.focus() == Focus::Home {
            if ui.home_field_editing() {
                // In the field: Down leaves it for the grid; Enter submits its
                // text as a navigation; everything else edits the text (egui).
                if matches!(key.kc, Keycode::Down) {
                    commands.push(AppCommand::Input(InputCommand::Nav(0, 1)));
                    return;
                }
                if !key.repeat && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
                    let text = ui.home_search_text();
                    if !text.trim().is_empty() {
                        commands.push(AppCommand::Menu(MenuAction::OpenUrl(text)));
                    }
                    return;
                }
            } else {
                let nav = match key.kc {
                    Keycode::Up => Some((0, -1)),
                    Keycode::Down => Some((0, 1)),
                    Keycode::Left => Some((-1, 0)),
                    Keycode::Right => Some((1, 0)),
                    _ => None,
                };
                if let Some((dx, dy)) = nav {
                    commands.push(AppCommand::Input(InputCommand::Nav(dx, dy)));
                    return;
                }
                if !key.repeat && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
                    commands.push(AppCommand::Input(InputCommand::Confirm(true)));
                    return;
                }
                // P toggles the focused tile's pin (Y's role) — routed as the same
                // intent the Y button emits.
                if ui.home_tile_selected() && matches!(key.kc, Keycode::P) {
                    commands.push(AppCommand::Input(InputCommand::Hints));
                    return;
                }
            }
        }

        // The speed-dial editor: arrows move the selection and Enter activates,
        // mirroring the start page. Its URL field, while it holds egui focus,
        // keeps typing/caret to the editor; Down leaves it for Add, Enter pins.
        if ui.focus() == Focus::DialEdit {
            if matches!(key.kc, Keycode::Escape) {
                commands.push(AppCommand::Input(InputCommand::Cancel));
                return;
            }
            if ui.dial_edit_field_editing() {
                // Up/Down leave the single-line field for the grid above / the
                // Add button below (left/right stay as caret movement in egui).
                if matches!(key.kc, Keycode::Up) {
                    commands.push(AppCommand::Input(InputCommand::Nav(0, -1)));
                    return;
                }
                if matches!(key.kc, Keycode::Down) {
                    commands.push(AppCommand::Input(InputCommand::Nav(0, 1)));
                    return;
                }
                if !key.repeat && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
                    let text = ui.dial_edit_input();
                    if !text.trim().is_empty() {
                        commands.push(AppCommand::Menu(MenuAction::DialAdd(text)));
                    }
                    return;
                }
            } else {
                let nav = match key.kc {
                    Keycode::Up => Some((0, -1)),
                    Keycode::Down => Some((0, 1)),
                    Keycode::Left => Some((-1, 0)),
                    Keycode::Right => Some((1, 0)),
                    _ => None,
                };
                if let Some((dx, dy)) = nav {
                    commands.push(AppCommand::Input(InputCommand::Nav(dx, dy)));
                    return;
                }
                if !key.repeat && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
                    commands.push(AppCommand::Input(InputCommand::Confirm(true)));
                    return;
                }
                // Delete the focused tile (X's role), routed as the same intent.
                if matches!(key.kc, Keycode::Delete | Keycode::Backspace)
                    && ui.dial_edit_tile().is_some()
                {
                    commands.push(AppCommand::Input(InputCommand::ToggleOsk));
                    return;
                }
            }
        }

        let overlay = matches!(ui.focus(), Focus::Osk | Focus::Hints);
        let typing = browser.text_input_focused()
            || ui.address_bar_focused()
            || ui.home_field_editing()
            || ui.dial_edit_field_editing();
        if let Some(action) = self.lookup(key, overlay, typing) {
            action.push_tap(commands);
            return;
        }

        browser.handle_input(servo::InputEvent::Keyboard(into_servo(key)));
    }

    /// Resolve a key event against the `[keyboard]` bindings, applying the
    /// firing rules: `nav_*` steps need an open overlay (and, unlike the other
    /// shortcuts, auto-repeat while held); plain bindings (no Ctrl/Alt) are
    /// muted while anything editable has focus, so they can't hijack typing.
    fn lookup(&self, key: &KeyEvent, overlay: bool, typing: bool) -> Option<Action> {
        let (action, plain) = self.bindings.lookup(key.kc, key.keymod)?;
        let fire = if action.is_nav() {
            overlay
        } else {
            !key.repeat && (!plain || !typing)
        };
        fire.then_some(action)
    }
}

fn into_servo(key: &KeyEvent) -> servo::KeyboardEvent {
    super::sdl2_servo::into_keyboard_event(key.kc, key.sc, key.keymod, key.pressed, key.repeat)
}
