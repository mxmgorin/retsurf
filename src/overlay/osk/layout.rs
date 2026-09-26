//! The keyboard's keys and its built-in layouts: the character rows each
//! language defines, the fixed frame around them, the named keys behind Fn, and
//! the wheel's character groups.

use super::wheel::WHEEL_SECTORS;
use crate::config::FaceLabels;
use std::collections::HashMap;
use std::sync::LazyLock;

/// One wheel layer: [`WHEEL_SECTORS`] groups of four characters, each in
/// [`super::wheel::FACES`] order.
pub(super) type WheelLayer = [&'static str; WHEEL_SECTORS];

/// The wheel's digits layer, shared by every layout. With [`DIGITS_SHIFTED`] it
/// holds every symbol a letters layer may lack.
pub(super) static DIGITS: WheelLayer = [
    "1234", "5678", "90.,", "?!:;", "-_/@", "(')\"", "+*=%", "`#&$",
];

/// [`DIGITS`] under Shift, slot by slot, so no symbol shifts into another's.
pub(super) static DIGITS_SHIFTED: WheelLayer = [
    "1234", "5678", "90><", "?!:;", "-|\\@", "[`]~", "{^}%", "`№&$",
];

/// The [`DIGITS`] slot that types the layout's key left of 1, as a keyboard does.
pub(super) const LEFT_OF_ONE: char = '`';

#[derive(Clone, Copy, PartialEq, Debug)]
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
    /// Empty the field being typed into.
    Clear,
    Hide,
    /// Swap between the characters and [`NAMED_ROWS`]; labeled with the one it
    /// leads to.
    Fn,
    /// `label` is the cell, `name` the `keyboard_types` spelling it sends.
    Named {
        label: &'static str,
        name: &'static str,
    },
}

impl Key {
    /// The gamepad button that directly triggers this key (the router's
    /// mapping), shown as a corner badge so the shortcuts are discoverable;
    /// keys without a dedicated button use D-pad + **A**.
    pub fn button_hint(self, face: FaceLabels) -> Option<&'static str> {
        match self {
            Key::Backspace => Some(face.x),
            Key::Space => Some(face.y),
            Key::Shift => Some("L2"),
            Key::Enter => Some("R2"),
            Key::Hide => Some(face.b),
            _ => None,
        }
    }
}

use Key::*;

/// The keys no character grid can carry, laid out like a keyboard's. Names are
/// `NamedKey` spellings; their `code` comes from [`code_for_named`].
pub(super) static NAMED_ROWS: &[&[(&str, &str)]] = &[
    &[
        ("Esc", "Escape"),
        ("F1", "F1"),
        ("F2", "F2"),
        ("F3", "F3"),
        ("F4", "F4"),
        ("F5", "F5"),
        ("F6", "F6"),
        ("F7", "F7"),
        ("F8", "F8"),
        ("F9", "F9"),
        ("F10", "F10"),
        ("F11", "F11"),
        ("F12", "F12"),
    ],
    // The navigation cluster in its usual 3x2.
    &[("Ins", "Insert"), ("Home", "Home"), ("PgUp", "PageUp")],
    &[("Del", "Delete"), ("End", "End"), ("PgDn", "PageDown")],
    // Bare modifiers, which games bind to run and crouch. Not the map's
    // `shift`/`ctrl`/`alt` flags: those qualify another key.
    &[
        ("Shift", "Shift"),
        ("Ctrl", "Control"),
        ("Alt", "Alt"),
        ("Meta", "Meta"),
    ],
];

/// The row every grid ends with. The keyboard is anchored to the bottom, so
/// keeping it identical leaves these keys put when Fn swaps what is above.
fn frame_row() -> Vec<Key> {
    vec![Lang, Fn, Clear, Space, Left, Up, Down, Right, Hide]
}

/// [`NAMED_ROWS`] over that same frame row; identical for every `Osk`, so it is
/// built once.
pub(super) static NAMED_KEYS: LazyLock<Vec<Vec<Key>>> = LazyLock::new(|| {
    NAMED_ROWS
        .iter()
        .map(|row| {
            row.iter()
                .map(|(label, name)| Named { label, name })
                .collect()
        })
        .chain([frame_row()])
        .collect()
});

/// A built-in layout's data: the four character rows between the fixed frame
/// keys, each mirrored by its shifted variant (position by position).
pub(super) struct LayoutDef {
    /// Config name (matched case-insensitively) and the Lang key's label.
    pub(super) name: &'static str,
    rows: [&'static str; 4],
    shift_rows: [&'static str; 4],
    /// The wheel's letters layer; [`DIGITS`] is the other one.
    pub(super) letters: WheelLayer,
}

/// The built-in layouts, selectable via `[osk] layouts` in the config. Adding
/// a language is adding an entry here: four rows of characters arranged like
/// the physical keyboard, plus their shifted forms.
pub(super) static LAYOUTS: &[LayoutDef] = &[
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
        letters: [
            "abcd", "efgh", "ijkl", "mnop", "qrst", "uvwx", "yz.,", "/:-@",
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
        // Thirty-three letters do not fit one layer; ё takes the digits'
        // left-of-1 slot.
        letters: [
            "абвг", "дежз", "ийкл", "мноп", "рсту", "фхцч", "шщъы", "ьэюя",
        ],
    },
];

/// A ready-to-use layout: the full key grid (character rows wrapped in the
/// fixed frame) and the Shift mapping for non-letter characters.
pub struct Layout {
    /// Shown on the Lang key.
    pub name: &'static str,
    pub(super) keys: Vec<Vec<Key>>,
    shift_map: HashMap<char, char>,
    pub(super) letters: &'static WheelLayer,
    left_of_one: char,
}

impl Layout {
    /// Wrap a definition's character rows in the fixed frame: Backspace top
    /// right, Enter at the home-row right, Shift around the bottom letter row,
    /// Space along the bottom with the arrow cluster after it.
    pub(super) fn build(def: &'static LayoutDef) -> Self {
        let chars = |r: usize| def.rows[r].chars().map(Char);
        let keys = vec![
            chars(0).chain([Backspace]).collect(),
            [Tab].into_iter().chain(chars(1)).collect(),
            [Caps].into_iter().chain(chars(2)).chain([Enter]).collect(),
            [Shift].into_iter().chain(chars(3)).chain([Shift]).collect(),
            frame_row(),
        ];
        let mut shift_map = HashMap::new();
        for (row, shifted) in def.rows.iter().zip(def.shift_rows) {
            shift_map.extend(row.chars().zip(shifted.chars()));
        }
        Self {
            name: def.name,
            keys,
            shift_map,
            letters: &def.letters,
            left_of_one: def.rows[0]
                .chars()
                .next()
                .expect("a layout's number row is never empty"),
        }
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

    /// The character the `index`th face of wheel group `sector` types. Shift and
    /// Caps change letters' case; Shift also picks [`DIGITS_SHIFTED`].
    pub fn wheel_char(
        &self,
        digits: bool,
        (sector, index): (usize, usize),
        shift: bool,
        caps: bool,
    ) -> char {
        let nth = |layer: &WheelLayer| {
            layer[sector]
                .chars()
                .nth(index)
                .expect("every wheel group holds one character per face")
        };
        let c = match digits {
            false => nth(self.letters),
            true if nth(&DIGITS) == LEFT_OF_ONE => self.left_of_one,
            true if shift => nth(&DIGITS_SHIFTED),
            true => nth(&DIGITS),
        };
        match c.is_alphabetic() {
            true => self.resolve_char(c, shift, caps),
            false => c,
        }
    }
}
