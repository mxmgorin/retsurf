//! The two edge strips: Quick Access, the actions at the right edge, and Quick
//! Menu, the places to go at the left. Each has one table per mode, so what a
//! game hides from the browser and back is data rather than branches.
//! Quick rows are [`crate::overlay::settings`] rows flagged `quick`: the same
//! label, values and config field, stepped live here instead of on close. The
//! central router ([`crate::app`]) drives it; [`crate::ui`] renders it.

use crate::browser::TabMode;
use crate::config::AppConfig;
use crate::event::bindings::Action;
use crate::overlay::menu::Section;
use crate::overlay::settings::{self, Settings};

/// Which strip is up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strip {
    /// Actions on the page or the game, at the right edge.
    QuickAccess,
    /// Places to go, at the left edge.
    QuickMenu,
}

/// Which tab modes a quick row shows in: the browser's (a page or its reader
/// view) or the game's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modes {
    Browser,
    Game,
}

impl Modes {
    pub fn includes(self, mode: TabMode) -> bool {
        match self {
            Modes::Browser => mode != TabMode::Game,
            Modes::Game => mode == TabMode::Game,
        }
    }
}

/// One row of the panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Turn Game Mode on.
    Enter,
    /// A settings row flagged `quick`, by its index in the settings table.
    Quick(usize),
    /// Run a bindable action, as its gesture would.
    Run(Action),
    /// Open the lists menu on a section.
    List(Section),
    /// The input map the game runs.
    InputMap,
    /// Summon the on-screen keyboard; it types into the page.
    Osk,
    /// Open the settings screen, on the tab the mode suggests.
    Settings,
    /// Turn Game Mode off.
    Exit,
    /// Close retsurf, on a second press (see [`QuickAccess::confirm_quit`]).
    Quit,
}

/// A table row: an entry, or where the mode's `quick` settings rows go.
#[derive(Clone, Copy)]
enum Slot {
    Row(Entry),
    QuickRows,
}

/// Quick Access in the browser. B closes a strip, so no row goes back.
const ACCESS_BROWSER: &[Slot] = &[
    Slot::Row(Entry::Enter),
    Slot::Row(Entry::Run(Action::Reader)),
    Slot::Row(Entry::Run(Action::Bookmark)),
    Slot::QuickRows,
];

/// Quick Access over a game: what changes it, then the way out.
const ACCESS_GAME: &[Slot] = &[
    Slot::QuickRows,
    Slot::Row(Entry::InputMap),
    Slot::Row(Entry::Osk),
    Slot::Row(Entry::Exit),
];

/// Quick Menu in the browser: where to go, so home leads.
const MENU_BROWSER: &[Slot] = &[
    Slot::Row(Entry::Run(Action::Home)),
    Slot::Row(Entry::List(Section::Tabs)),
    Slot::Row(Entry::List(Section::Bookmarks)),
    Slot::Row(Entry::List(Section::History)),
    Slot::Row(Entry::List(Section::Downloads)),
    Slot::Row(Entry::Settings),
    Slot::Row(Entry::Quit),
];

/// Quick Menu over a game: only what leaves the game where it is.
const MENU_GAME: &[Slot] = &[Slot::Row(Entry::Settings), Slot::Row(Entry::Quit)];

impl Entry {
    /// The row's label. No trailing ellipsis: that marks a row which asks for
    /// something before it acts.
    pub fn label(self) -> &'static str {
        match self {
            Entry::Enter => "Enter game mode",
            Entry::Quick(i) => Settings::fields()[i].quick_label(),
            Entry::Run(Action::Reader) => "Enter reader view",
            Entry::Run(action) => action.label(),
            Entry::List(section) => section.label(),
            Entry::InputMap => "Input map",
            // Not "Keyboard": a map has a `[keyboard]` table of physical keys.
            Entry::Osk => "On-screen keyboard",
            Entry::Settings => "Settings",
            Entry::Exit => "Exit game mode",
            Entry::Quit => "Quit retsurf",
        }
    }
}

/// The entries `strip` shows in `mode`, in its table's order.
fn entries(strip: Strip, mode: TabMode) -> Vec<Entry> {
    let game = mode == TabMode::Game;
    let layout = match (strip, game) {
        (Strip::QuickAccess, false) => ACCESS_BROWSER,
        (Strip::QuickAccess, true) => ACCESS_GAME,
        (Strip::QuickMenu, false) => MENU_BROWSER,
        (Strip::QuickMenu, true) => MENU_GAME,
    };
    let quick = |i: usize| {
        Settings::fields()[i]
            .quick
            .is_some_and(|modes| modes.includes(mode))
    };
    let mut rows = Vec::new();
    for &slot in layout {
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
    strip: Strip,
    mode: TabMode,
    rows: Vec<Entry>,
    /// Each row's value as last read from the config (empty for rows with none).
    values: Vec<String>,
    selected: usize,
    /// The Quit row was pressed once; any move disarms it.
    quit_armed: bool,
}

impl QuickAccess {
    pub fn new() -> Self {
        Self {
            visible: false,
            strip: Strip::QuickAccess,
            mode: TabMode::Page,
            rows: Vec::new(),
            values: Vec::new(),
            selected: 0,
            quit_armed: false,
        }
    }

    /// Show `strip`'s rows for the current mode, highlighting the first.
    pub fn open(&mut self, strip: Strip, mode: TabMode, config: &AppConfig) {
        self.visible = true;
        self.strip = strip;
        self.mode = mode;
        self.rows = entries(strip, mode);
        self.selected = 0;
        self.quit_armed = false;
        self.values = vec![String::new(); self.rows.len()];
        self.refresh(config);
    }

    /// Show `value` beside `entry`, for a row whose state the config does not
    /// hold (a page's bookmark, the tab count).
    pub fn set_value(&mut self, entry: Entry, value: String) {
        if let Some(at) = self.rows.iter().position(|row| *row == entry) {
            self.values[at] = value;
        }
    }

    /// Re-read the quick rows' values from `config`; values from
    /// [`Self::set_value`] stay.
    pub fn refresh(&mut self, config: &AppConfig) {
        for (row, value) in self.rows.iter().zip(&mut self.values) {
            if let Entry::Quick(i) = *row {
                *value = settings::value_of(&Settings::fields()[i].kind, config);
            }
        }
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn strip(&self) -> Strip {
        self.strip
    }

    /// Move the highlight by `dy` rows, past either end to the other: the strips
    /// are short enough that the far row is nearer the other way round.
    pub fn move_sel(&mut self, dy: i32) {
        self.selected = crate::list::wrap(self.selected, dy, self.rows.len());
        self.quit_armed = false;
    }

    pub fn select(&mut self, index: usize) {
        if index < self.rows.len() && index != self.selected {
            self.selected = index;
            self.quit_armed = false;
        }
    }

    /// A on the Quit row: arms it, or `true` when it already was.
    pub fn confirm_quit(&mut self) -> bool {
        std::mem::replace(&mut self.quit_armed, true)
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn rows(&self) -> &[Entry] {
        &self.rows
    }

    /// Row `index`'s label: [`Entry::label`], but a view the tab is in reads as
    /// the way out.
    pub fn label(&self, index: usize) -> &'static str {
        match self.rows.get(index) {
            Some(Entry::Run(Action::Reader)) if self.mode == TabMode::Reader => "Exit reader view",
            Some(row) => row.label(),
            None => "",
        }
    }

    pub fn value(&self, index: usize) -> &str {
        if self.quit_armed && self.rows.get(index) == Some(&Entry::Quit) {
            return "press again to quit";
        }
        self.values.get(index).map_or("", String::as_str)
    }

    pub fn row(&self) -> Entry {
        self.rows[self.selected]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(mode: TabMode) -> QuickAccess {
        let mut panel = QuickAccess::new();
        panel.open(Strip::QuickAccess, mode, &AppConfig::default());
        panel
    }

    #[test]
    fn the_highlight_starts_on_the_first_row_and_wraps_at_the_ends() {
        let mut panel = open(TabMode::Game);
        assert_eq!(panel.label(0), "View");
        panel.move_sel(-1);
        assert_eq!(panel.row(), Entry::Exit);
        panel.move_sel(1);
        assert_eq!(panel.label(panel.selected()), "View");
        let mut panel = open(TabMode::Page);
        assert_eq!(panel.row(), Entry::Enter);
        panel.move_sel(-1);
        assert_eq!(panel.label(panel.selected()), "Ad blocker");
    }

    #[test]
    fn each_mode_lists_its_own_rows() {
        let game = entries(Strip::QuickAccess, TabMode::Game);
        let browser = entries(Strip::QuickAccess, TabMode::Page);
        assert!(game.contains(&Entry::Exit) && !game.contains(&Entry::Enter));
        assert!(browser.contains(&Entry::Enter) && !browser.contains(&Entry::Exit));
        assert!(!browser.contains(&Entry::Osk));
        let has = |rows: &[Entry], label| rows.iter().any(|row| row.label() == label);
        assert!(has(&game, "View") && !has(&browser, "View"));
        assert!(has(&browser, "Page theme") && !has(&game, "Page theme"));
        assert!(!game
            .iter()
            .any(|row| matches!(row, Entry::Run(_) | Entry::List(_))));
        // Actions in Quick Access, places to go in Quick Menu.
        let door = |row: &Entry| {
            matches!(
                row,
                Entry::List(_) | Entry::Settings | Entry::Quit | Entry::Run(Action::Home)
            )
        };
        assert!(!browser.iter().any(door));
        let menu = entries(Strip::QuickMenu, TabMode::Page);
        assert!(menu.iter().all(door));
        assert_eq!(
            entries(Strip::QuickMenu, TabMode::Game),
            [Entry::Settings, Entry::Quit]
        );
        assert!(!game.contains(&Entry::Settings));
        for row in game.iter().chain(&browser).chain(&menu) {
            assert!(!row.label().ends_with("..."), "{row:?}");
        }
    }

    #[test]
    fn inside_the_reader_view_its_row_leaves_it() {
        let reader_row = |panel: &QuickAccess| {
            let at = panel
                .rows()
                .iter()
                .position(|row| *row == Entry::Run(Action::Reader))
                .expect("the browser rows carry the reader view");
            panel.label(at)
        };
        assert_eq!(reader_row(&open(TabMode::Page)), "Enter reader view");
        let panel = open(TabMode::Reader);
        assert_eq!(reader_row(&panel), "Exit reader view");
        assert_eq!(panel.rows(), open(TabMode::Page).rows());
    }

    #[test]
    fn quitting_takes_a_second_press() {
        let mut panel = QuickAccess::new();
        panel.open(Strip::QuickMenu, TabMode::Page, &AppConfig::default());
        panel.move_sel(-1);
        assert_eq!(panel.row(), Entry::Quit);
        assert!(!panel.confirm_quit());
        assert_eq!(panel.value(panel.selected()), "press again to quit");
        panel.move_sel(-1);
        panel.move_sel(1);
        assert!(!panel.confirm_quit());
        assert!(panel.confirm_quit());
    }

    /// Regression: stepping View blanked the input map's value.
    #[test]
    fn a_refresh_keeps_the_values_set_beside_it() {
        let mut panel = open(TabMode::Game);
        panel.set_value(Entry::InputMap, "Keyboard (WASD)".to_string());
        panel.refresh(&AppConfig::default());
        let at = panel
            .rows()
            .iter()
            .position(|row| *row == Entry::InputMap)
            .expect("the game's strip carries the input map");
        assert_eq!(panel.value(at), "Keyboard (WASD)");
    }

    #[test]
    fn a_quick_row_shows_the_config_value() {
        let panel = open(TabMode::Game);
        let at = panel
            .rows()
            .iter()
            .position(|row| row.label() == "View")
            .expect("the view is a quick row in the mode");
        assert_eq!(panel.value(at), "Page");
    }
}
