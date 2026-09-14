//! The Game Mode profile editor: one row per pad, showing what it sends and
//! letting the pad itself change it. The device this is for has no keyboard and
//! no file manager, so a profile it cannot edit here is a profile it cannot edit
//! at all; the file stays the fuller interface ([`crate::event::game_profile`]).
//!
//! Sticks, physical keys and layers are the file's — they need names this screen
//! has no room to pick.

use inputbind::Pad;

/// The targets a row can take without leaving the list: the specials, plus
/// "a key", which hands over to the on-screen keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    /// Unbound: the button reaches the page as a gamepad button, as it would
    /// with no profile at all.
    Unbound,
    Passthrough,
    Click,
    None,
}

impl Special {
    /// What Left/Right steps through, ahead of the keys.
    pub const ALL: [Special; 4] = [
        Special::Unbound,
        Special::Passthrough,
        Special::Click,
        Special::None,
    ];

    /// How the file spells it; `Unbound` has no spelling — it is the absence of
    /// an entry.
    pub fn text(self) -> Option<&'static str> {
        match self {
            Special::Unbound => None,
            Special::Passthrough => Some("passthrough"),
            Special::Click => Some("click"),
            Special::None => Some("none"),
        }
    }

    fn from_text(text: Option<&str>) -> Option<Special> {
        Special::ALL
            .into_iter()
            .find(|special| special.text() == text)
    }
}

/// Every pad the editor lists: Select carries the Game Mode menu in every
/// profile, so it is not one of them.
pub fn sources() -> Vec<Pad> {
    Pad::ALL
        .into_iter()
        .filter(|pad| *pad != Pad::Select)
        .collect()
}

pub struct GameEdit {
    visible: bool,
    selected: usize,
    /// Whether the on-screen keyboard is up to pick this row's key.
    picking: bool,
    /// What it picked, as the file spells it — read and cleared by the app.
    picked: Option<String>,
    /// Whether anything changed, so an untouched visit writes no file.
    dirty: bool,
}

impl GameEdit {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected: 0,
            picking: false,
            picked: None,
            dirty: false,
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.selected = 0;
        self.picking = false;
        self.picked = None;
        self.dirty = false;
    }

    /// Close it, reporting whether the profile needs writing.
    pub fn close(&mut self) -> bool {
        self.visible = false;
        self.picking = false;
        std::mem::take(&mut self.dirty)
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The pad the focused row stands for.
    pub fn source(&self) -> Pad {
        sources()[self.selected.min(sources().len() - 1)]
    }

    pub fn move_sel(&mut self, dy: i32) {
        let last = sources().len() as i32 - 1;
        self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
    }

    pub fn select(&mut self, index: usize) {
        if index < sources().len() {
            self.selected = index;
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Whether the keyboard is up picking a key for the focused row.
    pub fn picking(&self) -> bool {
        self.picking
    }

    pub fn set_picking(&mut self, on: bool) {
        self.picking = on;
    }

    /// The slot the on-screen keyboard writes its pick into.
    pub fn picked_mut(&mut self) -> &mut Option<String> {
        &mut self.picked
    }

    /// Take a pick the keyboard left, if any.
    pub fn take_picked(&mut self) -> Option<String> {
        self.picked.take()
    }

    /// Step the focused row through the specials, from whatever it says now.
    /// Returns what to write, or `None` to unbind — a key target steps out of
    /// the list at its nearest end, since the list cannot name every key.
    pub fn step_special(&self, current: Option<&str>, delta: i32) -> Option<&'static str> {
        let at = Special::from_text(current).unwrap_or(Special::Unbound);
        let index = Special::ALL.iter().position(|s| *s == at).unwrap_or(0);
        let count = Special::ALL.len() as i32;
        let next = (index as i32 + delta).rem_euclid(count) as usize;
        Special::ALL[next].text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Select is the one pad no profile may name, so the editor must not offer
    /// a row that would be refused on save.
    #[test]
    fn the_reserved_pad_is_not_a_row() {
        let sources = sources();
        assert!(!sources.contains(&Pad::Select));
        assert_eq!(sources.len(), Pad::COUNT - 1);
    }

    #[test]
    fn the_specials_step_in_a_ring_from_wherever_the_row_is() {
        let edit = GameEdit::new();
        assert_eq!(edit.step_special(None, 1), Some("passthrough"));
        assert_eq!(edit.step_special(Some("passthrough"), 1), Some("click"));
        assert_eq!(edit.step_special(Some("none"), 1), None);
        assert_eq!(edit.step_special(None, -1), Some("none"));
        // A key target is not in the ring; stepping enters it at an end.
        assert_eq!(edit.step_special(Some("Space"), 1), Some("passthrough"));
    }

    #[test]
    fn the_highlight_stops_at_the_ends() {
        let mut edit = GameEdit::new();
        edit.open();
        edit.move_sel(-1);
        assert_eq!(edit.selected(), 0);
        edit.move_sel(99);
        assert_eq!(edit.selected(), sources().len() - 1);
    }
}
