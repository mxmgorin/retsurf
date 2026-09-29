//! Quick Access: the action panel at the screen's left edge, one for both modes.
//! Its entries come from one table, each marked with the modes it shows in, so
//! what a game hides from the browser and back is data rather than branches.
//! Quick rows are [`crate::overlay::settings`] rows flagged `quick`: the same
//! label, values and config field, stepped live here instead of on close. The
//! central router ([`crate::app`]) drives it; [`crate::ui`] renders it.

use crate::config::AppConfig;
use crate::overlay::settings::{self, Settings};

/// Which modes an entry shows in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modes {
    Browser,
    Game,
    Both,
}

impl Modes {
    pub const fn includes(self, in_game_mode: bool) -> bool {
        match self {
            Modes::Browser => !in_game_mode,
            Modes::Game => in_game_mode,
            Modes::Both => true,
        }
    }
}

/// One row of the panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Turn Game Mode on.
    Enter,
    /// Close the panel over the running game. Not "Resume": the game never
    /// stopped (Servo exposes no suspend API).
    Back,
    /// A settings row flagged `quick`, by its index in the settings table.
    Quick(usize),
    /// Summon the on-screen keyboard; it types into the page.
    Osk,
    /// Open the settings screen, on the tab the mode suggests.
    Settings,
    /// Turn Game Mode off. Last, as far as the list allows from where an
    /// accidental open lands.
    Exit,
}

/// Where the flagged settings rows go in [`LAYOUT`].
#[derive(Clone, Copy)]
enum Slot {
    Row(Entry),
    QuickRows,
}

/// The panel top to bottom. Inside the mode it is a pause menu: the way back
/// leads, the way out ends it.
const LAYOUT: &[(Slot, Modes)] = &[
    (Slot::Row(Entry::Enter), Modes::Browser),
    (Slot::Row(Entry::Back), Modes::Game),
    (Slot::QuickRows, Modes::Both),
    (Slot::Row(Entry::Osk), Modes::Game),
    (Slot::Row(Entry::Settings), Modes::Both),
    (Slot::Row(Entry::Exit), Modes::Game),
];

impl Entry {
    /// The row's label. No trailing ellipsis: that marks a row which asks for
    /// something before it acts.
    pub fn label(self) -> &'static str {
        match self {
            Entry::Enter => "Enter game mode",
            Entry::Back => "Back to game",
            Entry::Quick(i) => Settings::fields()[i].label,
            // Not "Keyboard": a map has a `[keyboard]` table of physical
            // keys, and this is the one on screen.
            Entry::Osk => "On-screen keyboard",
            Entry::Settings => "Settings",
            Entry::Exit => "Exit game mode",
        }
    }
}

/// The entries `in_game_mode` shows, in [`LAYOUT`] order.
fn entries(in_game_mode: bool) -> Vec<Entry> {
    let quick = |i: usize| {
        Settings::fields()[i]
            .quick
            .is_some_and(|modes| modes.includes(in_game_mode))
    };
    let mut rows = Vec::new();
    for &(slot, modes) in LAYOUT {
        if !modes.includes(in_game_mode) {
            continue;
        }
        match slot {
            Slot::Row(entry) => rows.push(entry),
            Slot::QuickRows => rows.extend(
                (0..Settings::fields().len())
                    .filter(|&i| quick(i))
                    .map(Entry::Quick),
            ),
        }
    }
    rows
}

pub struct QuickAccess {
    pub visible: bool,
    in_game_mode: bool,
    rows: Vec<Entry>,
    /// Each row's value as last read from the config (empty for rows with none).
    values: Vec<String>,
    selected: usize,
}

impl QuickAccess {
    pub fn new() -> Self {
        Self {
            visible: false,
            in_game_mode: false,
            rows: Vec::new(),
            values: Vec::new(),
            selected: 0,
        }
    }

    /// Show the rows for the current mode, highlighting the first: back to the
    /// game inside it, so an accidental open is one A-press from where it was.
    pub fn open(&mut self, in_game_mode: bool, config: &AppConfig) {
        self.visible = true;
        self.in_game_mode = in_game_mode;
        self.rows = entries(in_game_mode);
        self.selected = 0;
        self.refresh(config);
    }

    /// Re-read the quick rows' values after `config` changed.
    pub fn refresh(&mut self, config: &AppConfig) {
        self.values = self
            .rows
            .iter()
            .map(|row| match *row {
                Entry::Quick(i) => settings::value_of(&Settings::fields()[i].kind, config),
                _ => String::new(),
            })
            .collect();
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn in_game_mode(&self) -> bool {
        self.in_game_mode
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

    pub fn rows(&self) -> &[Entry] {
        &self.rows
    }

    pub fn value(&self, index: usize) -> &str {
        self.values.get(index).map_or("", String::as_str)
    }

    pub fn row(&self) -> Entry {
        self.rows[self.selected]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(in_game_mode: bool) -> QuickAccess {
        let mut panel = QuickAccess::new();
        panel.open(in_game_mode, &AppConfig::default());
        panel
    }

    #[test]
    fn the_highlight_starts_on_the_first_row_and_stops_at_the_ends() {
        let mut panel = open(true);
        assert_eq!(panel.row(), Entry::Back);
        panel.move_sel(99);
        assert_eq!(panel.row(), Entry::Exit);
        panel.move_sel(-99);
        assert_eq!(panel.row(), Entry::Back);
        let mut panel = open(false);
        assert_eq!(panel.row(), Entry::Enter);
        panel.move_sel(99);
        assert_eq!(panel.row(), Entry::Settings);
    }

    /// Each mode offers its own way across and nothing that only makes sense in
    /// the other; the game's view is a quick row inside the mode only.
    #[test]
    fn each_mode_lists_its_own_rows() {
        let game = entries(true);
        let browser = entries(false);
        assert!(game.contains(&Entry::Exit) && !game.contains(&Entry::Enter));
        assert!(browser.contains(&Entry::Enter) && !browser.contains(&Entry::Exit));
        assert!(!browser.contains(&Entry::Back) && !browser.contains(&Entry::Osk));
        let view = |rows: &[Entry]| rows.iter().any(|row| row.label() == "View");
        assert!(view(&game) && !view(&browser));
        for row in game.iter().chain(&browser) {
            assert!(!row.label().ends_with("..."), "{row:?}");
        }
    }

    /// A quick row reads its value from the config it was refreshed with.
    #[test]
    fn a_quick_row_shows_the_config_value() {
        let panel = open(true);
        let at = panel
            .rows()
            .iter()
            .position(|row| row.label() == "View")
            .expect("the view is a quick row in the mode");
        assert_eq!(panel.value(at), "Page");
    }
}
