//! On-screen keyboard for gamepad text entry, styled after the Steam Deck's.
//! Opened with the **X** button; keys are typed into the address bar (which also
//! doubles as search). Owned and rendered by [`crate::ui`], which drives it from
//! gamepad input. Beyond grid navigation (D-pad + **A**), the common keys have
//! direct shortcuts: **X** backspace, **Y** space, **L2** shift, **R2** enter.

use crate::app::AppCommand;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::event::sdl2_servo::{char_keyboard_event, named_keyboard_event};
use keyboard_types::{Code, NamedKey};

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
    /// Current input language indicator (display-only for now).
    Lang,
    /// Hides the keyboard.
    Hide,
}

use Key::*;

/// Keyboard layout, row by row, arranged like a real keyboard: Backspace top
/// right, Enter at the home-row right, Shift at the bottom-letter-row left,
/// Space along the bottom with the arrow cluster after it. Rows may differ in
/// length.
pub static LAYOUT: &[&[Key]] = &[
    &[
        Char('`'), Char('1'), Char('2'), Char('3'), Char('4'), Char('5'),
        Char('6'), Char('7'), Char('8'), Char('9'), Char('0'),
        Char('-'), Char('='), Backspace,
    ],
    &[
        Tab, Char('q'), Char('w'), Char('e'), Char('r'), Char('t'),
        Char('y'), Char('u'), Char('i'), Char('o'), Char('p'),
        Char('['), Char(']'), Char('\\'),
    ],
    &[
        Caps, Char('a'), Char('s'), Char('d'), Char('f'), Char('g'),
        Char('h'), Char('j'), Char('k'), Char('l'), Char(';'), Char('\''), Enter,
    ],
    &[
        Shift, Char('z'), Char('x'), Char('c'), Char('v'),
        Char('b'), Char('n'), Char('m'), Char(','), Char('.'), Char('/'), Shift,
    ],
    &[Lang, Space, Left, Up, Down, Right, Hide],
];

/// The character produced by a key when Shift is held, mirroring a US keyboard
/// (number row → symbols, letters → uppercase).
pub fn shift_char(c: char) -> char {
    match c {
        '1' => '!', '2' => '@', '3' => '#', '4' => '$', '5' => '%',
        '6' => '^', '7' => '&', '8' => '*', '9' => '(', '0' => ')',
        '-' => '_', '=' => '+', '/' => '?', '.' => '>', ',' => '<', ';' => ':',
        '`' => '~', '[' => '{', ']' => '}', '\\' => '|', '\'' => '"',
        c => c.to_ascii_uppercase(),
    }
}

/// The character a `Char` key produces given the modifier state. Shift shifts
/// everything (US-keyboard style); Caps Lock only flips letter case, so for a
/// letter the case is `shift XOR caps`.
pub fn resolve_char(c: char, shift: bool, caps: bool) -> char {
    if c.is_ascii_alphabetic() {
        if shift ^ caps {
            c.to_ascii_uppercase()
        } else {
            c
        }
    } else if shift {
        shift_char(c)
    } else {
        c
    }
}

/// Label to show on a key, honoring the current shift/caps state.
pub fn key_label(key: Key, shift: bool, caps: bool) -> String {
    match key {
        Char(c) => resolve_char(c, shift, caps).to_string(),
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
        Lang => "EN".to_string(),
        Hide => "Hide".to_string(),
    }
}

/// On-screen keyboard state: visibility, the selected cell, and shift/caps.
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
}

impl Osk {
    pub fn new() -> Self {
        Self {
            visible: false,
            caps: false,
            shift_held: false,
            shift_once: false,
            row: 0,
            col: 0,
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
        LAYOUT[self.row][self.col]
    }

    /// Move the selection by one cell; `dx`/`dy` are -1, 0 or 1. The column is
    /// clamped to the (possibly shorter) destination row.
    fn move_sel(&mut self, dx: i32, dy: i32) {
        let rows = LAYOUT.len() as i32;
        self.row = (self.row as i32 + dy).clamp(0, rows - 1) as usize;
        let cols = LAYOUT[self.row].len() as i32;
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
                let ch = resolve_char(c, shift, self.caps);
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
            // Display-only for now (single language).
            Lang => {}
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
