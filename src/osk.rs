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
    /// Toggle the sticky shift.
    Shift,
    /// Submit (load the address bar or send Enter), then hide.
    Enter,
    /// Move the selection by one cell (`dx`, `dy` ∈ -1..=1).
    Move(i32, i32),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Key {
    Char(char),
    Space,
    Backspace,
    Shift,
    Left,
    Up,
    Down,
    Right,
    Go,
}

use Key::*;

/// Keyboard layout, row by row, arranged like a real keyboard: Backspace top
/// right, Enter (Go) at the home-row right, Shift at the bottom-letter-row left,
/// Space along the bottom with the arrow cluster after it. Rows may differ in
/// length.
pub static LAYOUT: &[&[Key]] = &[
    &[
        Char('1'), Char('2'), Char('3'), Char('4'), Char('5'),
        Char('6'), Char('7'), Char('8'), Char('9'), Char('0'), Backspace,
    ],
    &[
        Char('q'), Char('w'), Char('e'), Char('r'), Char('t'),
        Char('y'), Char('u'), Char('i'), Char('o'), Char('p'),
    ],
    &[
        Char('a'), Char('s'), Char('d'), Char('f'), Char('g'),
        Char('h'), Char('j'), Char('k'), Char('l'), Go,
    ],
    &[
        Shift, Char('z'), Char('x'), Char('c'), Char('v'),
        Char('b'), Char('n'), Char('m'), Char(','), Char('.'), Char('/'),
    ],
    &[Char(':'), Char('-'), Space, Left, Up, Down, Right],
];

/// The character produced by a key when Shift is held, mirroring a US keyboard
/// (number row → symbols, letters → uppercase).
pub fn shift_char(c: char) -> char {
    match c {
        '1' => '!', '2' => '@', '3' => '#', '4' => '$', '5' => '%',
        '6' => '^', '7' => '&', '8' => '*', '9' => '(', '0' => ')',
        '-' => '_', '/' => '?', '.' => '>', ',' => '<', ':' => ':',
        c => c.to_ascii_uppercase(),
    }
}

/// Label to show on a key, honoring the current shift state.
pub fn key_label(key: Key, shift: bool) -> String {
    match key {
        Char(c) if shift => shift_char(c).to_string(),
        Char(c) => c.to_string(),
        Space => "Space".to_string(),
        Backspace => "Bksp".to_string(),
        Shift => "Shift".to_string(),
        Left => "<".to_string(),
        Up => "^".to_string(),
        Down => "v".to_string(),
        Right => ">".to_string(),
        Go => "Go".to_string(),
    }
}

/// On-screen keyboard state: visibility, the selected cell, and shift.
pub struct Osk {
    pub visible: bool,
    pub shift: bool,
    row: usize,
    col: usize,
}

impl Osk {
    pub fn new() -> Self {
        Self {
            visible: false,
            shift: false,
            row: 0,
            col: 0,
        }
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
            OskCommand::Show => self.visible = true,
            OskCommand::Hide => self.visible = false,
            OskCommand::Activate => self.activate(to_address_bar, browser, commands),
            OskCommand::Backspace => self.backspace(to_address_bar, browser),
            OskCommand::Space => self.type_space(to_address_bar, browser),
            OskCommand::Shift => self.toggle_shift(),
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
            Shift => self.toggle_shift(),
            Char(c) => {
                let c = if self.shift { shift_char(c) } else { c };
                input_char(to_address_bar, c, self.shift, browser);
            }
            Space => self.type_space(to_address_bar, browser),
            Backspace => self.backspace(to_address_bar, browser),
            // Arrow keys are sent to the focused page element only; the address
            // bar is append-only here, so they do nothing there.
            Left if !to_address_bar => send_named(browser, NamedKey::ArrowLeft, Code::ArrowLeft),
            Right if !to_address_bar => send_named(browser, NamedKey::ArrowRight, Code::ArrowRight),
            Up if !to_address_bar => send_named(browser, NamedKey::ArrowUp, Code::ArrowUp),
            Down if !to_address_bar => send_named(browser, NamedKey::ArrowDown, Code::ArrowDown),
            Left | Right | Up | Down => {}
            Go => self.enter(to_address_bar, browser, commands),
        }
    }

    /// Toggle the sticky Shift state (the **Shift** key or **L2**).
    fn toggle_shift(&mut self) {
        self.shift = !self.shift;
    }

    /// Type a space (the **Space** key or **Y**).
    fn type_space(&self, to_address_bar: bool, browser: &AppBrowser) {
        input_char(to_address_bar, ' ', self.shift, browser);
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
