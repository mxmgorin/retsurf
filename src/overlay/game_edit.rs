//! The Game Mode profile editor: one row per pad, showing what it sends and
//! letting the pad itself change it. The device this is for has no keyboard and
//! no file manager, so a profile it cannot edit here is a profile it cannot edit
//! at all; the file stays the fuller interface ([`crate::event::game_profile`]).
//!
//! Sticks, physical keys and layers are the file's — they need names this screen
//! has no room to pick.

use inputbind::Pad;

/// What a row can be set to. One list, so nothing about a row is hidden behind a
/// verb the screen cannot show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Hands over to the on-screen keyboard, which picks the key itself.
    Key,
    Click,
    /// The page reads the button through the Gamepad API. An unbound button
    /// does the same thing; this is the file saying so out loud, and the two
    /// are one entry here because they are one behaviour.
    Passthrough,
    /// Consumed and dropped: the button does nothing at all.
    Ignore,
}

impl Kind {
    /// Top to bottom, keys first — it is what a row is usually set to.
    pub const ALL: [Kind; 4] = [Kind::Key, Kind::Click, Kind::Passthrough, Kind::Ignore];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Key => "Key...",
            Kind::Click => "Mouse click",
            Kind::Passthrough => "Gamepad button",
            Kind::Ignore => "Ignore",
        }
    }

    /// How the file spells it; `Key` has none — the keyboard supplies it.
    pub fn text(self) -> Option<&'static str> {
        match self {
            Kind::Key => None,
            Kind::Click => Some("click"),
            Kind::Passthrough => Some("passthrough"),
            Kind::Ignore => Some("none"),
        }
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
    /// The row's kind list, open over it; `None` while the rows are.
    kind: Option<usize>,
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
            kind: None,
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
        self.kind = None;
        self.picking = false;
        self.picked = None;
        self.dirty = false;
    }

    /// Close it, reporting whether the profile needs writing.
    pub fn close(&mut self) -> bool {
        self.visible = false;
        self.kind = None;
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

    /// Move within whichever list is up.
    pub fn move_sel(&mut self, dy: i32) {
        match &mut self.kind {
            Some(at) => {
                let last = Kind::ALL.len() as i32 - 1;
                *at = (*at as i32 + dy).clamp(0, last) as usize;
            }
            None => {
                let last = sources().len() as i32 - 1;
                self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
            }
        }
    }

    /// The kind list this row has open, if any.
    pub fn kind_open(&self) -> Option<usize> {
        self.kind
    }

    /// Open it on the focused row, or close it again (B).
    pub fn open_kinds(&mut self) {
        self.kind = Some(0);
    }

    pub fn close_kinds(&mut self) {
        self.kind = None;
    }

    /// The kind the list is on.
    pub fn kind(&self) -> Option<Kind> {
        self.kind.map(|at| Kind::ALL[at.min(Kind::ALL.len() - 1)])
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

    /// Every kind is on screen, and only the key one defers to the keyboard.
    #[test]
    fn a_row_can_be_set_to_anything_the_list_shows() {
        for kind in Kind::ALL {
            assert!(!kind.label().is_empty());
            assert_eq!(kind.text().is_none(), kind == Kind::Key);
        }
    }

    #[test]
    fn the_highlight_stops_at_the_ends_of_whichever_list_is_up() {
        let mut edit = GameEdit::new();
        edit.open();
        edit.move_sel(-1);
        assert_eq!(edit.selected(), 0);
        edit.move_sel(99);
        assert_eq!(edit.selected(), sources().len() - 1);
        // The kind list moves instead while it is open, and the row stays put.
        let row = edit.selected();
        edit.open_kinds();
        edit.move_sel(99);
        assert_eq!(edit.kind(), Some(Kind::Ignore));
        assert_eq!(edit.selected(), row);
    }
}
