//! Game Mode's own menu — the one screen the mode is reached through, in or out
//! of it. Inside, the game keeps running underneath (Servo exposes no suspend
//! API), so the list is short. The central router ([`crate::app`]) drives it
//! like any other overlay; [`crate::ui`] renders it.

/// The rows, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameRow {
    /// Close the menu: back to the game, or to the browser.
    Resume,
    /// The active pad mapping, cycled in place (A / Left / Right). Settable from
    /// outside the mode too, which is the point of opening the menu there.
    Profile,
    /// Summon the on-screen keyboard; it types into the page.
    TypeText,
    /// Enter or leave Game Mode — the only row whose action depends on state.
    Toggle,
}

impl GameRow {
    /// Top-to-bottom order, which is also the selection index.
    pub const ALL: [GameRow; 4] = [
        GameRow::Resume,
        GameRow::Profile,
        GameRow::TypeText,
        GameRow::Toggle,
    ];

    /// The row's label, which two rows word by state: outside the mode there is
    /// no game to resume, and the last row is the way in rather than out. The
    /// panel is titled GAME MODE, so that row needs no noun of its own.
    pub fn label(self, in_game_mode: bool) -> &'static str {
        match (self, in_game_mode) {
            (GameRow::Resume, true) => "Resume",
            (GameRow::Resume, false) => "Close",
            (GameRow::Profile, _) => "Profile",
            (GameRow::TypeText, _) => "Type text...",
            (GameRow::Toggle, true) => "Disable",
            (GameRow::Toggle, false) => "Enable",
        }
    }
}

pub struct GameMenu {
    pub visible: bool,
    selected: usize,
}

impl GameMenu {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected: 0,
        }
    }

    /// Show it, highlighting what the opener most likely wants: inside the mode
    /// Resume, so an accidental open over a running game is one A-press from
    /// gone; outside it the row that enters, which is what it was opened for.
    pub fn open(&mut self, in_game_mode: bool) {
        self.visible = true;
        self.selected = match in_game_mode {
            true => 0,
            false => GameRow::ALL.len() - 1,
        };
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move the highlight by `dy` rows (clamped to the ends, like the menu's).
    pub fn move_sel(&mut self, dy: i32) {
        let last = GameRow::ALL.len() as i32 - 1;
        self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
    }

    /// Focus a row by index (clicking it).
    pub fn select(&mut self, index: usize) {
        if index < GameRow::ALL.len() {
            self.selected = index;
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn row(&self) -> GameRow {
        GameRow::ALL[self.selected]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highlight_starts_where_the_opener_is_going_and_stops_at_the_ends() {
        let mut menu = GameMenu::new();
        menu.open(true);
        assert_eq!(menu.row(), GameRow::Resume);
        menu.move_sel(-1);
        assert_eq!(menu.row(), GameRow::Resume);
        menu.move_sel(99);
        assert_eq!(menu.row(), GameRow::Toggle);
        // Opened from outside the mode, entering is one A-press away.
        menu.close();
        menu.open(false);
        assert_eq!(menu.row(), GameRow::Toggle);
    }

    /// Only the two state-worded rows change, and every row is always labelled.
    #[test]
    fn the_labels_follow_the_mode() {
        for row in GameRow::ALL {
            assert!(!row.label(true).is_empty() && !row.label(false).is_empty());
            let same = row.label(true) == row.label(false);
            assert_eq!(same, matches!(row, GameRow::Profile | GameRow::TypeText));
        }
    }
}
