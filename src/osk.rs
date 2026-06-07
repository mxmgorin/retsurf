//! On-screen keyboard for gamepad text entry, styled after the Steam Deck's.
//! Opened with the **X** button; keys are typed into the address bar (which also
//! doubles as search). Driven by the gamepad; rendered by [`crate::ui`].

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

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn selected(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    pub fn current(&self) -> Key {
        LAYOUT[self.row][self.col]
    }

    /// Move the selection by one cell; `dx`/`dy` are -1, 0 or 1. The column is
    /// clamped to the (possibly shorter) destination row.
    pub fn move_sel(&mut self, dx: i32, dy: i32) {
        let rows = LAYOUT.len() as i32;
        self.row = (self.row as i32 + dy).clamp(0, rows - 1) as usize;
        let cols = LAYOUT[self.row].len() as i32;
        self.col = (self.col as i32 + dx).clamp(0, cols - 1) as usize;
    }
}
