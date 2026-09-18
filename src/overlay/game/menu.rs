//! Game Mode's own menu — the one screen the mode is reached through, in or out
//! of it. Inside, the game keeps running underneath (Servo exposes no suspend
//! API), so the list is short. The central router ([`crate::app`]) drives it
//! like any other overlay; [`crate::ui`] renders it.

/// The rows, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameRow {
    /// Enter or leave Game Mode — the only row whose action depends on state,
    /// and what the screen is mostly opened for, so it leads.
    Toggle,
    /// Close the menu: back to the game, or to the browser — either way, back
    /// to what the opener was doing.
    Resume,
    /// Open the input-map screens (see [`super::input_maps`]): which map both
    /// devices run, and everything that can be done to one. Reachable from
    /// outside the mode too, which is the point of opening the menu there.
    InputMap,
    /// Summon the on-screen keyboard; it types into the page.
    Osk,
}

impl GameRow {
    /// Top-to-bottom order, which is also the selection index.
    pub const ALL: [GameRow; 4] = [
        GameRow::Toggle,
        GameRow::Resume,
        GameRow::InputMap,
        GameRow::Osk,
    ];

    /// The row's label. Only the toggle words itself by state; the panel is
    /// titled GAME MODE, so it needs no noun of its own. No trailing ellipsis —
    /// that marks a row which asks for something before it acts.
    pub fn label(self, in_game_mode: bool) -> &'static str {
        match (self, in_game_mode) {
            (GameRow::Resume, _) => "Resume",
            // Singular: the value beside it is the map in use, and one map
            // covers both devices — hence input, not controller.
            (GameRow::InputMap, _) => "Input map",
            // Not "Keyboard": a map has a `[keyboard]` table of physical
            // keys, and this is the one on screen.
            (GameRow::Osk, _) => "On-screen keyboard",
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
    /// back and not from ending it; outside it, the row that enters.
    pub fn open(&mut self, in_game_mode: bool) {
        self.visible = true;
        let wanted = match in_game_mode {
            true => GameRow::Resume,
            false => GameRow::Toggle,
        };
        self.selected = GameRow::ALL
            .iter()
            .position(|row| *row == wanted)
            .expect("ALL lists every row");
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move the highlight by `dy` rows (clamped to the ends, like the menu's).
    pub fn move_sel(&mut self, dy: i32) {
        self.selected = crate::list::step(self.selected, dy, GameRow::ALL.len());
    }

    /// Focus a row by index.
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
        menu.move_sel(99);
        assert_eq!(menu.row(), GameRow::Osk);
        menu.move_sel(-99);
        assert_eq!(menu.row(), GameRow::Toggle);
        // Opened from outside the mode, entering is one A-press away — and an
        // accidental open inside it must not put leaving there instead.
        menu.close();
        menu.open(false);
        assert_eq!(menu.row(), GameRow::Toggle);
        menu.close();
        menu.open(true);
        assert_ne!(menu.row(), GameRow::Toggle);
    }

    /// One row words itself by state, and it is the one whose action does. No
    /// row here asks for anything, so none may trail off.
    #[test]
    fn the_labels_follow_the_mode() {
        for row in GameRow::ALL {
            assert!(!row.label(true).is_empty() && !row.label(false).is_empty());
            assert!(!row.label(true).ends_with("..."), "{row:?}");
            let same = row.label(true) == row.label(false);
            assert_eq!(same, row != GameRow::Toggle, "{row:?}");
        }
    }
}
