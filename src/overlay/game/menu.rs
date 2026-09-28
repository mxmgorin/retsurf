//! Game Mode's own menu — the one screen the mode is reached through, in or out
//! of it. Inside, the game keeps running underneath (Servo exposes no suspend
//! API), so the list is short. The central router ([`crate::app`]) drives it
//! like any other overlay; [`crate::ui`] renders it.

/// A row of the menu; which rows show depends on the mode (see [`GameMenu::open`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameRow {
    /// Turn Game Mode on; the reason to open the menu from outside it.
    Enter,
    /// Close the menu over the running game. Not "Resume": the game never
    /// stopped (Servo exposes no suspend API).
    Back,
    /// Open the input-map screens (see [`super::input_maps`]): which map both
    /// devices run, and everything that can be done to one. Reachable from
    /// outside the mode too, so the map can be set before a game.
    InputMap,
    /// Summon the on-screen keyboard; it types into the page.
    Osk,
    /// Turn Game Mode off. Last, as far as the list allows from where an
    /// accidental open lands.
    Exit,
}

/// Inside the mode: a pause menu, so the way back leads and the way out ends it.
const IN_MODE: &[GameRow] = &[
    GameRow::Back,
    GameRow::InputMap,
    GameRow::Osk,
    GameRow::Exit,
];

/// Outside it: entering, or setting the map up first. B closes, and the browser
/// has its own keyboard, so neither Back nor Osk has a use here.
const OUT_OF_MODE: &[GameRow] = &[GameRow::Enter, GameRow::InputMap];

impl GameRow {
    /// The row's label. No trailing ellipsis: that marks a row which asks for
    /// something before it acts.
    pub fn label(self) -> &'static str {
        match self {
            GameRow::Enter => "Enter game mode",
            GameRow::Back => "Back to game",
            // Singular: the value beside it is the map in use, and one map
            // covers both devices — hence input, not controller.
            GameRow::InputMap => "Input map",
            // Not "Keyboard": a map has a `[keyboard]` table of physical
            // keys, and this is the one on screen.
            GameRow::Osk => "On-screen keyboard",
            GameRow::Exit => "Exit game mode",
        }
    }
}

pub struct GameMenu {
    pub visible: bool,
    rows: &'static [GameRow],
    selected: usize,
}

impl GameMenu {
    pub fn new() -> Self {
        Self {
            visible: false,
            rows: OUT_OF_MODE,
            selected: 0,
        }
    }

    /// Show the rows for the current mode, highlighting the first: back to the
    /// game inside it, so an accidental open is one A-press from where it was.
    pub fn open(&mut self, in_game_mode: bool) {
        self.visible = true;
        self.rows = match in_game_mode {
            true => IN_MODE,
            false => OUT_OF_MODE,
        };
        self.selected = 0;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move the highlight by `dy` rows (clamped to the ends, like the menu's).
    pub fn move_sel(&mut self, dy: i32) {
        self.selected = crate::list::step(self.selected, dy, self.rows.len());
    }

    /// Focus a row by index.
    pub fn select(&mut self, index: usize) {
        if index < self.rows.len() {
            self.selected = index;
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn rows(&self) -> &'static [GameRow] {
        self.rows
    }

    pub fn row(&self) -> GameRow {
        self.rows[self.selected]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highlight_starts_on_the_first_row_and_stops_at_the_ends() {
        let mut menu = GameMenu::new();
        menu.open(true);
        assert_eq!(menu.row(), GameRow::Back);
        menu.move_sel(99);
        assert_eq!(menu.row(), GameRow::Exit);
        menu.move_sel(-99);
        assert_eq!(menu.row(), GameRow::Back);
        menu.close();
        menu.open(false);
        assert_eq!(menu.row(), GameRow::Enter);
        menu.move_sel(99);
        assert_eq!(menu.row(), GameRow::InputMap);
    }

    /// Each mode offers its own way across and nothing that only makes sense in
    /// the other; no row trails off into an ellipsis.
    #[test]
    fn each_mode_lists_its_own_rows() {
        assert!(IN_MODE.contains(&GameRow::Exit) && !IN_MODE.contains(&GameRow::Enter));
        assert!(OUT_OF_MODE.contains(&GameRow::Enter) && !OUT_OF_MODE.contains(&GameRow::Exit));
        assert!(!OUT_OF_MODE.contains(&GameRow::Back));
        for row in IN_MODE.iter().chain(OUT_OF_MODE) {
            assert!(!row.label().ends_with("..."), "{row:?}");
        }
    }
}
