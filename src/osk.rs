//! On-screen keyboard for gamepad text entry, styled after the Steam Deck's.
//! Opened with the **X** button; keys are typed into the address bar (which also
//! doubles as search). Driven by the gamepad; rendered by [`crate::ui`].

#[derive(Clone, Copy, PartialEq)]
pub enum Key {
    Char(char),
    Space,
    Backspace,
    Shift,
    Go,
}

use Key::*;

/// Keyboard layout, row by row. Rows may have different lengths.
pub static LAYOUT: &[&[Key]] = &[
    &[
        Char('1'), Char('2'), Char('3'), Char('4'), Char('5'),
        Char('6'), Char('7'), Char('8'), Char('9'), Char('0'),
    ],
    &[
        Char('q'), Char('w'), Char('e'), Char('r'), Char('t'),
        Char('y'), Char('u'), Char('i'), Char('o'), Char('p'),
    ],
    &[
        Char('a'), Char('s'), Char('d'), Char('f'), Char('g'),
        Char('h'), Char('j'), Char('k'), Char('l'), Char(':'),
    ],
    &[
        Shift, Char('z'), Char('x'), Char('c'), Char('v'),
        Char('b'), Char('n'), Char('m'), Char('.'), Backspace,
    ],
    &[Char('/'), Char('-'), Char('_'), Space, Go],
];

/// Label to show on a key, honoring the current shift state.
pub fn key_label(key: Key, shift: bool) -> String {
    match key {
        Char(c) if shift && c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        Char(c) => c.to_string(),
        Space => "Space".to_string(),
        Backspace => "Bksp".to_string(),
        Shift => "Shift".to_string(),
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
