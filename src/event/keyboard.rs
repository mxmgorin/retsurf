//! Translates raw keyboard input into [`AppCommand`]s — the keyboard
//! counterpart of [`crate::event::gamepad`]. It applies the firing rules over
//! the `[keyboard]` table: `nav_*` steps need an open overlay (and auto-repeat
//! while held), plain shortcuts (no Ctrl/Alt) are muted while anything editable
//! has focus, and the menu / hint overlays get their fixed keys first. Whatever
//! isn't consumed is forwarded to the page as a Servo keyboard event.

use crate::app::{AppCommand, InputCommand, MenuAction};
use crate::browser::AppBrowser;
use crate::event::bindings::Action;
use crate::ui::{AppUi, Focus};
use inputbind::sdl::{key_code, mods_for};
use inputbind::Bindings;
use sdl2::keyboard::{Keycode, Mod, Scancode};

/// One key edge as SDL reports it, bundled for resolution.
pub struct KeyEvent {
    pub kc: Keycode,
    pub sc: Scancode,
    pub keymod: Mod,
    pub repeat: bool,
    pub pressed: bool,
}

/// Resolve one key edge: overlay fixed keys first, then the shortcut table, and
/// finally fall through to the page.
pub fn on_key(
    key: &KeyEvent,
    bindings: &Bindings<Action>,
    ui: &AppUi,
    browser: &AppBrowser,
    commands: &mut Vec<AppCommand>,
) {
    // Android's hardware/gesture Back arrives as AC_BACK (the SDL trap-back
    // hint is set in lib.rs). Map it to the focus-aware Cancel intent so it
    // works in every context, swallowing both edges so nothing leaks to the page.
    if key.kc == Keycode::AcBack {
        if key.pressed && !key.repeat {
            commands.push(AppCommand::Input(InputCommand::Cancel));
        }
        return;
    }

    // A modal page prompt (select picker / JS dialog) captures the keyboard
    // first: Enter activates, Esc dismisses, the `nav_*` bindings move the
    // focus, and everything else is muted so a shortcut can't fire under the
    // modal. The on-screen keyboard stays above it — that's how a gamepad types
    // into `prompt()`.
    if ui.focus() == Focus::Prompt {
        if key.pressed {
            match key.kc {
                Keycode::Return | Keycode::KpEnter => {
                    commands.push(AppCommand::Input(InputCommand::Confirm(true)))
                }
                Keycode::Escape => commands.push(AppCommand::Input(InputCommand::Cancel)),
                _ => {
                    if let Some(action) = lookup(key, bindings, true, true) {
                        if action.is_nav() {
                            action.push_tap(commands);
                        }
                    }
                }
            }
        }
        return;
    }

    // While the menu is open it captures the keyboard wholesale — both edges, so
    // no stray release reaches the page either.
    if ui.menu.visible {
        if key.pressed {
            on_menu_key(key, bindings, commands);
        }
        return;
    }

    if key.pressed {
        on_key_down(key, bindings, ui, browser, commands);
    } else if ui.hints.visible && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
        // Hint mode times Enter as a tap-vs-hold gesture, so its release edge
        // decides (click vs open-in-new-tab) in the router rather than going to
        // the page like other key-ups.
        commands.push(AppCommand::Input(InputCommand::Confirm(false)));
    } else {
        browser.handle_input(servo::InputEvent::Keyboard(into_servo(key)));
    }
}

/// The menu owns the keyboard: Esc closes, Enter opens, Delete removes;
/// navigation and shortcuts go through the bindings.
fn on_menu_key(key: &KeyEvent, bindings: &Bindings<Action>, commands: &mut Vec<AppCommand>) {
    // The menu overlay covers everything, so nothing editable can hold focus —
    // `typing` is moot here.
    if let Some(action) = lookup(key, bindings, true, false) {
        action.push_tap(commands);
        return;
    }
    match key.kc {
        // Tab / Shift+Tab switch section (the Shoulder intent L1/R1 emit),
        // mirroring the settings overlay; one section per press.
        Keycode::Tab if !key.repeat => {
            let shift = key.keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
            commands.push(AppCommand::Input(InputCommand::Shoulder(if shift {
                -1
            } else {
                1
            })));
        }
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
    key: &KeyEvent,
    bindings: &Bindings<Action>,
    ui: &AppUi,
    browser: &AppBrowser,
    commands: &mut Vec<AppCommand>,
) {
    // Hint mode's fixed keys (its navigation comes from the `nav_*` bindings
    // below). Enter is a tap-vs-hold gesture timed in the router, so only its
    // first edge counts.
    if ui.hints.visible {
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
        // Keyboard-opened hint mode: letter keys type the on-badge hint code
        // (Vimium-style), so capture every letter here before the shortcut table
        // or the page sees it. Modified keys (Ctrl+R etc.) still fall through.
        let modified = key
            .keymod
            .intersects(Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LALTMOD | Mod::RALTMOD);
        if ui.hints.is_keyboard() && ui.hint_badges() && !key.repeat && !modified {
            if let Some(c) = letter_of(key.kc) {
                commands.push(AppCommand::Input(InputCommand::HintKey(c)));
                return;
            }
        }
    }

    // The start page: arrows move the selection and Enter activates — the same
    // intents the gamepad routes. While its search field holds keyboard focus,
    // typing/caret/Enter belong to the text editor; only Down leaves the field.
    if ui.focus() == Focus::Home {
        if ui.home_field_editing() {
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
            if let Some((dx, dy)) = arrow_nav(key.kc) {
                commands.push(AppCommand::Input(InputCommand::Nav(dx, dy)));
                return;
            }
            if !key.repeat && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
                commands.push(AppCommand::Input(InputCommand::Confirm(true)));
                return;
            }
            // P toggles the focused tile's pin (Y's role).
            if ui.home_tile_selected() && matches!(key.kc, Keycode::P) {
                commands.push(AppCommand::Input(InputCommand::Hints));
                return;
            }
        }
    }

    // The speed-dial editor mirrors the start page. Its URL field, while it
    // holds egui focus, keeps typing/caret to the editor; Up/Down leave it
    // (grid / Pin settings), Enter pins.
    if ui.focus() == Focus::DialEdit {
        if matches!(key.kc, Keycode::Escape) {
            commands.push(AppCommand::Input(InputCommand::Cancel));
            return;
        }
        // Tab / Shift+Tab reorder the focused pin, like L1/R1 (and like the
        // section switch they drive in the menu and settings).
        if matches!(key.kc, Keycode::Tab) {
            if !key.repeat {
                let shift = key.keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
                let delta = if shift { -1 } else { 1 };
                commands.push(AppCommand::Input(InputCommand::Shoulder(delta)));
            }
            return;
        }
        // Ctrl+arrows fall through to the bindings (`prev`/`next`, i.e. the same
        // reorder); only the plain ones move the selection.
        let ctrl = key.keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        if ui.dial_edit_field_editing() {
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
            if let Some((dx, dy)) = arrow_nav(key.kc).filter(|_| !ctrl) {
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

    // The settings overlay: plain arrows move the selection (Up/Down) and adjust
    // the focused value (Left/Right); Tab / Shift+Tab and Ctrl+Left/Right switch
    // section; Enter activates; Esc saves and closes. No text field can hold egui
    // focus here (typing goes through the OSK), so arrows are never caret moves.
    if ui.focus() == Focus::Settings {
        if matches!(key.kc, Keycode::Tab) {
            if !key.repeat {
                let shift = key.keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
                let delta = if shift { -1 } else { 1 };
                commands.push(AppCommand::Input(InputCommand::Shoulder(delta)));
            }
            return;
        }
        let ctrl = key.keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        let nav = if ctrl { None } else { arrow_nav(key.kc) };
        if let Some((dx, dy)) = nav {
            commands.push(AppCommand::Input(InputCommand::Nav(dx, dy)));
            return;
        }
        if !key.repeat && matches!(key.kc, Keycode::Return | Keycode::KpEnter) {
            commands.push(AppCommand::Input(InputCommand::Confirm(true)));
            return;
        }
        if matches!(key.kc, Keycode::Escape) {
            commands.push(AppCommand::Input(InputCommand::Cancel));
            return;
        }
    }

    // Overlays whose navigation comes from the `nav_*` bindings, so vim hjkl
    // works there and not just the arrows the fixed handlers above catch.
    let overlay = matches!(ui.focus(), Focus::Osk | Focus::Hints | Focus::Settings);
    let typing = browser.text_input_focused()
        || ui.address_bar_focused()
        || ui.home_field_editing()
        || ui.dial_edit_field_editing();
    if let Some(action) = lookup(key, bindings, overlay, typing) {
        action.push_tap(commands);
        return;
    }

    browser.handle_input(servo::InputEvent::Keyboard(into_servo(key)));
}

/// Resolve a key event against the `[keyboard]` bindings, applying the firing
/// rules: `nav_*` steps need an open overlay (and, unlike the other shortcuts,
/// auto-repeat while held); plain bindings (no Ctrl/Alt) are muted while
/// anything editable has focus, so they can't hijack typing.
fn lookup(
    key: &KeyEvent,
    bindings: &Bindings<Action>,
    overlay: bool,
    typing: bool,
) -> Option<Action> {
    let mods = mods_for(key.kc, key.keymod);
    let action = bindings.key(key_code(key.kc), mods)?;
    let plain = mods.is_plain();
    let fire = if action.is_nav() {
        overlay
    } else {
        !key.repeat && (!plain || !typing)
    };
    fire.then_some(action)
}

fn into_servo(key: &KeyEvent) -> servo::KeyboardEvent {
    super::sdl2_servo::into_keyboard_event(key.kc, key.sc, key.keymod, key.pressed, key.repeat)
}

/// The lowercase letter `a`..=`z` a keycode stands for, else `None`. SDL letter
/// keycodes are their ASCII lowercase value, so the range maps straight across.
fn letter_of(kc: Keycode) -> Option<char> {
    let v = kc.into_i32();
    (('a' as i32)..=('z' as i32))
        .contains(&v)
        .then_some(v as u8 as char)
}

/// The (dx, dy) overlay-navigation step an arrow keycode stands for, else `None`.
fn arrow_nav(kc: Keycode) -> Option<(i32, i32)> {
    match kc {
        Keycode::Up => Some((0, -1)),
        Keycode::Down => Some((0, 1)),
        Keycode::Left => Some((-1, 0)),
        Keycode::Right => Some((1, 0)),
        _ => None,
    }
}
