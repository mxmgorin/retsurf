//! On-screen keyboard for gamepad text entry, styled after the Steam Deck's.
//! Opened with the **X** button; keys are typed into the address bar (which also
//! doubles as search). Owned and rendered by [`crate::ui`], which drives it from
//! gamepad input. Beyond grid navigation (D-pad + **A**), the common keys have
//! direct shortcuts: **X** backspace, **Y** space, **L2** shift, **R2** enter.
//!
//! Layouts are built in ([`LAYOUTS`]: QWERTY and ЙЦУКЕН so far); the config's
//! `[osk] layouts` list picks which are enabled, and the **Lang** key cycles
//! through them in that order. Each layout defines only the four character
//! rows — the frame (Tab, Caps, Enter, Shift, Space, arrows) is fixed.

use crate::app::AppCommand;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::config::OskConfig;
use crate::event::sdl2_servo::{char_keyboard_event, named_keyboard_event};
use keyboard_types::{Code, NamedKey};
use std::collections::HashMap;

/// An operation on the on-screen keyboard. The router produces these from
/// contextual buttons (A→Activate, X→Show/Backspace, B→Hide), the stick (→Move)
/// and the dedicated keys (Space/Shift/Enter), then dispatches them via
/// [`Osk::handle`].
#[derive(Clone, Copy)]
pub enum OskCommand {
    /// Show the keyboard.
    Show,
    /// Hide the keyboard.
    Hide,
    /// Apply the selected key.
    Activate,
    /// Delete the character before the caret.
    Backspace,
    /// Type a space.
    Space,
    /// Set the held-Shift modifier (L2): `true` while the trigger is pulled.
    Shift(bool),
    /// Submit (load the address bar or send Enter), then hide.
    Enter,
    /// Move the selection by one cell (`dx`, `dy` ∈ -1..=1).
    Move(i32, i32),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Key {
    Char(char),
    Tab,
    Caps,
    Space,
    Backspace,
    Shift,
    Left,
    Up,
    Down,
    Right,
    Enter,
    /// Cycle to the next enabled layout; labeled with the current one's name.
    Lang,
    /// Hides the keyboard.
    Hide,
}

use Key::*;

/// A built-in layout's data: the four character rows between the fixed frame
/// keys, each mirrored by its shifted variant (position by position).
struct LayoutDef {
    /// Config name (matched case-insensitively) and the Lang key's label.
    name: &'static str,
    rows: [&'static str; 4],
    shift_rows: [&'static str; 4],
}

/// The built-in layouts, selectable via `[osk] layouts` in the config. Adding
/// a language is adding an entry here: four rows of characters arranged like
/// the physical keyboard, plus their shifted forms.
static LAYOUTS: &[LayoutDef] = &[
    LayoutDef {
        name: "en",
        rows: [
            "`1234567890-=",
            "qwertyuiop[]\\",
            "asdfghjkl;'",
            "zxcvbnm,./",
        ],
        shift_rows: [
            "~!@#$%^&*()_+",
            "QWERTYUIOP{}|",
            "ASDFGHJKL:\"",
            "ZXCVBNM<>?",
        ],
    },
    LayoutDef {
        name: "ru",
        rows: [
            "ё1234567890-=",
            "йцукенгшщзхъ\\",
            "фывапролджэ",
            "ячсмитьбю.",
        ],
        shift_rows: [
            "Ё!\"№;%:?*()_+",
            "ЙЦУКЕНГШЩЗХЪ/",
            "ФЫВАПРОЛДЖЭ",
            "ЯЧСМИТЬБЮ,",
        ],
    },
];

/// A ready-to-use layout: the full key grid (character rows wrapped in the
/// fixed frame) and the Shift mapping for non-letter characters.
pub struct Layout {
    /// Shown on the Lang key.
    pub name: &'static str,
    keys: Vec<Vec<Key>>,
    shift_map: HashMap<char, char>,
}

impl Layout {
    /// Wrap a definition's character rows in the fixed frame: Backspace top
    /// right, Enter at the home-row right, Shift around the bottom letter row,
    /// Space along the bottom with the arrow cluster after it.
    fn build(def: &LayoutDef) -> Self {
        let chars = |r: usize| def.rows[r].chars().map(Char);
        let keys = vec![
            chars(0).chain([Backspace]).collect(),
            [Tab].into_iter().chain(chars(1)).collect(),
            [Caps].into_iter().chain(chars(2)).chain([Enter]).collect(),
            [Shift].into_iter().chain(chars(3)).chain([Shift]).collect(),
            vec![Lang, Space, Left, Up, Down, Right, Hide],
        ];
        let mut shift_map = HashMap::new();
        for (row, shifted) in def.rows.iter().zip(def.shift_rows) {
            shift_map.extend(row.chars().zip(shifted.chars()));
        }
        Self {
            name: def.name,
            keys,
            shift_map,
        }
    }

    /// The key grid, row by row (rows may differ in length).
    pub fn keys(&self) -> &[Vec<Key>] {
        &self.keys
    }

    /// The character a `Char` key produces given the modifier state. Letters
    /// flip case by `shift XOR caps` (Caps Lock only affects case); anything
    /// else shifts through the layout's mapping.
    pub fn resolve_char(&self, c: char, shift: bool, caps: bool) -> char {
        if c.is_alphabetic() {
            if shift ^ caps {
                c.to_uppercase().next().unwrap_or(c)
            } else {
                c
            }
        } else if shift {
            self.shift_map.get(&c).copied().unwrap_or(c)
        } else {
            c
        }
    }
}

/// On-screen keyboard state: visibility, the selected cell, shift/caps, and
/// the enabled layouts.
pub struct Osk {
    pub visible: bool,
    pub caps: bool,
    /// Held Shift from the L2 trigger — a momentary modifier, on while pulled.
    shift_held: bool,
    /// One-shot Shift from the on-screen Shift key — armed by a tap, consumed by
    /// the next character.
    shift_once: bool,
    row: usize,
    col: usize,
    /// The enabled layouts in Lang-cycle order; never empty.
    layouts: Vec<Layout>,
    /// Index of the active layout.
    lang: usize,
}

impl Osk {
    pub fn new(cfg: &OskConfig) -> Self {
        let mut layouts: Vec<Layout> = cfg
            .layouts
            .iter()
            .filter_map(|id| {
                let def = LAYOUTS.iter().find(|d| d.name.eq_ignore_ascii_case(id));
                if def.is_none() {
                    let known: Vec<_> = LAYOUTS.iter().map(|d| d.name).collect();
                    log::warn!("osk: unknown layout `{id}` (available: {known:?}); skipping");
                }
                def.map(Layout::build)
            })
            .collect();
        // The keyboard is the only text input on a handheld — never come up
        // without one.
        if layouts.is_empty() {
            layouts.push(Layout::build(&LAYOUTS[0]));
        }

        Self {
            visible: false,
            caps: false,
            shift_held: false,
            shift_once: false,
            row: 0,
            col: 0,
            layouts,
            lang: 0,
        }
    }

    /// The active layout.
    pub fn layout(&self) -> &Layout {
        &self.layouts[self.lang]
    }

    /// Label to show on a key, honoring the current shift/caps state and the
    /// active layout.
    pub fn key_label(&self, key: Key) -> String {
        match key {
            Char(c) => self
                .layout()
                .resolve_char(c, self.shift(), self.caps)
                .to_string(),
            Tab => "Tab".to_string(),
            Caps => "Caps".to_string(),
            Space => "Space".to_string(),
            Backspace => "Bksp".to_string(),
            Shift => "Shift".to_string(),
            Left => "<".to_string(),
            Up => "^".to_string(),
            Down => "v".to_string(),
            Right => ">".to_string(),
            Enter => "Enter".to_string(),
            Lang => self.layout().name.to_uppercase(),
            Hide => "Hide".to_string(),
        }
    }

    /// Whether Shift is currently in effect: the L2 trigger is held, or the
    /// on-screen Shift key was tapped and not yet consumed.
    pub fn shift(&self) -> bool {
        self.shift_held || self.shift_once
    }

    /// Dispatch an [`OskCommand`]. `to_address_bar` selects where typed input
    /// goes: the egui address bar, or the web page's focused element.
    pub fn handle(
        &mut self,
        cmd: OskCommand,
        to_address_bar: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        match cmd {
            OskCommand::Show => {
                self.visible = true;
                // Start fresh: a trigger released while hidden never sends its
                // release edge, so don't carry a stale held-Shift into a session.
                self.shift_held = false;
                self.shift_once = false;
            }
            OskCommand::Hide => self.visible = false,
            OskCommand::Activate => self.activate(to_address_bar, browser, commands),
            OskCommand::Backspace => self.backspace(to_address_bar, browser),
            OskCommand::Space => self.type_space(to_address_bar, browser),
            OskCommand::Shift(held) => self.shift_held = held,
            OskCommand::Enter => self.enter(to_address_bar, browser, commands),
            OskCommand::Move(dx, dy) => self.move_sel(dx, dy),
        }
    }

    pub fn selected(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    fn current(&self) -> Key {
        self.layout().keys[self.row][self.col]
    }

    /// Move the selection by one cell; `dx`/`dy` are -1, 0 or 1. The column is
    /// clamped to the (possibly shorter) destination row.
    fn move_sel(&mut self, dx: i32, dy: i32) {
        let rows = self.layout().keys.len() as i32;
        self.row = (self.row as i32 + dy).clamp(0, rows - 1) as usize;
        let cols = self.layout().keys[self.row].len() as i32;
        self.col = (self.col as i32 + dx).clamp(0, cols - 1) as usize;
    }

    /// Apply the selected key. Input is routed to the focused text field: the egui
    /// address bar when `to_address_bar`, otherwise the web page's focused element
    /// (via Servo keyboard events).
    fn activate(
        &mut self,
        to_address_bar: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        match self.current() {
            // The on-screen Shift key arms a one-shot Shift (toggle so a mis-tap
            // can be undone); L2 is the held modifier and lives in `shift_held`.
            Shift => self.shift_once = !self.shift_once,
            Caps => self.caps = !self.caps,
            Char(c) => {
                let shift = self.shift();
                let ch = self.layout().resolve_char(c, shift, self.caps);
                input_char(to_address_bar, ch, shift, browser);
                // Consume the one-shot; the held (L2) modifier stays as-is.
                self.shift_once = false;
            }
            Space => self.type_space(to_address_bar, browser),
            Backspace => self.backspace(to_address_bar, browser),
            // Tab and the arrow keys are sent to the focused page element only;
            // the address bar is append-only here, so they do nothing there.
            Tab if !to_address_bar => send_named(browser, NamedKey::Tab, Code::Tab),
            Left if !to_address_bar => send_named(browser, NamedKey::ArrowLeft, Code::ArrowLeft),
            Right if !to_address_bar => send_named(browser, NamedKey::ArrowRight, Code::ArrowRight),
            Up if !to_address_bar => send_named(browser, NamedKey::ArrowUp, Code::ArrowUp),
            Down if !to_address_bar => send_named(browser, NamedKey::ArrowDown, Code::ArrowDown),
            Tab | Left | Right | Up | Down => {}
            Enter => self.enter(to_address_bar, browser, commands),
            Lang => {
                self.lang = (self.lang + 1) % self.layouts.len();
                // The frame is fixed but rows differ in length across
                // layouts — keep the selection on a valid cell.
                self.col = self.col.min(self.layout().keys[self.row].len() - 1);
            }
            Hide => self.visible = false,
        }
    }

    /// Type a space (the **Space** key or **Y**).
    fn type_space(&self, to_address_bar: bool, browser: &AppBrowser) {
        input_char(to_address_bar, ' ', self.shift(), browser);
    }

    /// Delete the character before the caret (the **Backspace** key or **X**).
    fn backspace(&self, to_address_bar: bool, browser: &AppBrowser) {
        if to_address_bar {
            browser.get_state_mut().get_location_mut().pop();
        } else {
            send_named(browser, NamedKey::Backspace, Code::Backspace);
        }
    }

    /// Submit: load the address bar or send Enter to the page, then hide (the
    /// **Go** key or **R2**).
    fn enter(
        &mut self,
        to_address_bar: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if to_address_bar {
            commands.push(AppCommand::Browser(BrowserCommand::Load));
        } else {
            send_named(browser, NamedKey::Enter, Code::Enter);
        }
        self.visible = false;
    }
}

fn input_char(to_address_bar: bool, c: char, shift: bool, browser: &AppBrowser) {
    if to_address_bar {
        browser.get_state_mut().get_location_mut().push(c);
    } else {
        browser.handle_input(servo::InputEvent::Keyboard(char_keyboard_event(c, shift, true)));
        browser.handle_input(servo::InputEvent::Keyboard(char_keyboard_event(c, shift, false)));
    }
}

fn send_named(browser: &AppBrowser, key: NamedKey, code: Code) {
    browser.handle_input(servo::InputEvent::Keyboard(named_keyboard_event(key, code, true)));
    browser.handle_input(servo::InputEvent::Keyboard(named_keyboard_event(key, code, false)));
}
