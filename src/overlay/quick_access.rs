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
    /// Reload the game's page, on a second press.
    Reload,
    /// Turn Game Mode off.
    Exit,
    /// Close retsurf, on a second press (see [`QuickAccess::confirm`]).
    Quit,
}

/// A table row: an entry, a `quick` settings row by its quick label, or the
/// mode's remaining `quick` rows.
#[derive(Clone, Copy)]
enum Slot {
    Row(Entry),
    Quick(&'static str),
    QuickRows,
}

/// Quick Access in the browser. B closes a strip, so no row goes back.
const ACCESS_BROWSER: &[Slot] = &[
    Slot::Row(Entry::Enter),
    Slot::Row(Entry::Run(Action::Bookmark)),
    Slot::QuickRows,
    Slot::Row(Entry::Run(Action::Reader)),
    Slot::Quick("Page theme"),
];

/// Quick Access over a game: reload first, what changes it, then the way out.
const ACCESS_GAME: &[Slot] = &[
    Slot::Row(Entry::Reload),
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
            Entry::Run(Action::Reader) => "Reader view",
            Entry::Run(action) => action.label(),
            Entry::List(section) => section.label(),
            Entry::InputMap => "Input map",
            // Not "Keyboard": a map has a `[keyboard]` table of physical keys.
            Entry::Osk => "On-screen keyboard",
            Entry::Settings => "Settings",
            Entry::Reload => "Reload",
            Entry::Exit => "Exit game mode",
            Entry::Quit => "Quit retsurf",
        }
    }

    /// What the row shows once armed, for a row that acts on a second press
    /// (see [`QuickAccess::confirm`]); `None` for a row that acts at once.
    pub fn confirm_hint(self) -> Option<&'static str> {
        match self {
            Entry::Quit => Some("press again to quit"),
            Entry::Reload => Some("press again to reload"),
            _ => None,
        }
    }

    /// Whether Left/Right step the row's value.
    pub fn steps(self) -> bool {
        matches!(
            self,
            Entry::Quick(_) | Entry::InputMap | Entry::Run(Action::Reader)
        )
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
        let field = &Settings::fields()[i];
        field.shown() && field.quick.is_some_and(|modes| modes.includes(mode))
    };
    let named = |i: usize| {
        let label = Settings::fields()[i].quick_label();
        layout
            .iter()
            .any(|slot| matches!(slot, Slot::Quick(l) if *l == label))
    };
    let mut rows = Vec::new();
    for &slot in layout {
        match slot {
            Slot::Row(entry) => rows.push(entry),
            Slot::Quick(label) => rows.extend(
                (0..Settings::fields().len())
                    .filter(|&i| quick(i) && Settings::fields()[i].quick_label() == label)
                    .map(Entry::Quick),
            ),
            Slot::QuickRows => rows.extend(
                (0..Settings::fields().len())
                    .filter(|&i| quick(i) && !named(i))
                    .map(Entry::Quick),
            ),
        }
    }
    rows
}

pub struct QuickAccess {
    pub visible: bool,
    strip: Strip,
    rows: Vec<Entry>,
    /// Each row's value as last read from the config (empty for rows with none).
    values: Vec<String>,
    selected: usize,
    /// A two-press row pressed once; any move disarms it.
    armed: Option<Entry>,
}

impl QuickAccess {
    pub fn new() -> Self {
        Self {
            visible: false,
            strip: Strip::QuickAccess,
            rows: Vec::new(),
            values: Vec::new(),
            selected: 0,
            armed: None,
        }
    }

    /// Show `strip`'s rows for the current mode, highlighting the first.
    pub fn open(&mut self, strip: Strip, mode: TabMode, config: &AppConfig) {
        self.visible = true;
        self.strip = strip;
        self.rows = entries(strip, mode);
        self.selected = 0;
        self.armed = None;
        self.values = vec![String::new(); self.rows.len()];
        self.refresh(config);
        let reader = settings::on_off(mode == TabMode::Reader);
        self.set_value(Entry::Run(Action::Reader), reader.to_string());
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
        self.armed = None;
    }

    pub fn select(&mut self, index: usize) {
        if index < self.rows.len() && index != self.selected {
            self.selected = index;
            self.armed = None;
        }
    }

    /// A on a two-press row: arms it, or `true` when it already was.
    pub fn confirm(&mut self, row: Entry) -> bool {
        self.armed.replace(row) == Some(row)
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn rows(&self) -> &[Entry] {
        &self.rows
    }

    pub fn label(&self, index: usize) -> &'static str {
        self.rows.get(index).map_or("", |row| row.label())
    }

    pub fn value(&self, index: usize) -> &str {
        if let Some(hint) = self.rows.get(index).and_then(|row| {
            (self.armed == Some(*row))
                .then(|| row.confirm_hint())
                .flatten()
        }) {
            return hint;
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
        assert_eq!(panel.row(), Entry::Reload);
        assert_eq!(panel.label(1), "Scaling");
        panel.move_sel(-1);
        assert_eq!(panel.row(), Entry::Exit);
        panel.move_sel(1);
        assert_eq!(panel.row(), Entry::Reload);
        let mut panel = open(TabMode::Page);
        assert_eq!(panel.row(), Entry::Enter);
        let labels: Vec<_> = (0..panel.rows().len()).map(|i| panel.label(i)).collect();
        assert_eq!(
            labels,
            [
                "Enter game mode",
                "Bookmark",
                "User agent",
                "Ad blocker",
                "Reader view",
                "Page theme"
            ]
        );
        panel.move_sel(-1);
        assert_eq!(panel.label(panel.selected()), "Page theme");
    }

    #[test]
    fn each_mode_lists_its_own_rows() {
        let game = entries(Strip::QuickAccess, TabMode::Game);
        let browser = entries(Strip::QuickAccess, TabMode::Page);
        assert!(game.contains(&Entry::Exit) && !game.contains(&Entry::Enter));
        assert!(browser.contains(&Entry::Enter) && !browser.contains(&Entry::Exit));
        assert!(!browser.contains(&Entry::Osk));
        let has = |rows: &[Entry], label| rows.iter().any(|row| row.label() == label);
        assert!(has(&game, "Scaling") && !has(&browser, "Scaling"));
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
    fn the_reader_row_shows_whether_the_tab_is_in_it() {
        let reader_row = |panel: &QuickAccess| {
            let at = panel
                .rows()
                .iter()
                .position(|row| *row == Entry::Run(Action::Reader))
                .expect("the browser rows carry the reader view");
            (panel.label(at), panel.value(at).to_string())
        };
        assert_eq!(
            reader_row(&open(TabMode::Page)),
            ("Reader view", "Off".into())
        );
        let panel = open(TabMode::Reader);
        assert_eq!(reader_row(&panel), ("Reader view", "On".into()));
        assert_eq!(panel.rows(), open(TabMode::Page).rows());
    }

    #[test]
    fn quitting_takes_a_second_press() {
        let mut panel = QuickAccess::new();
        panel.open(Strip::QuickMenu, TabMode::Page, &AppConfig::default());
        panel.move_sel(-1);
        assert_eq!(panel.row(), Entry::Quit);
        assert!(!panel.confirm(Entry::Quit));
        assert_eq!(panel.value(panel.selected()), "press again to quit");
        panel.move_sel(-1);
        panel.move_sel(1);
        assert!(!panel.confirm(Entry::Quit));
        assert!(panel.confirm(Entry::Quit));
    }

    /// Reload arms like Quit, and arming one row never confirms another.
    #[test]
    fn reloading_takes_a_second_press_of_its_own() {
        let mut panel = open(TabMode::Game);
        assert_eq!(panel.row(), Entry::Reload);
        assert!(!panel.confirm(Entry::Reload));
        assert_eq!(panel.value(panel.selected()), "press again to reload");
        assert!(!panel.confirm(Entry::Quit));
        assert!(!panel.confirm(Entry::Reload));
        assert!(panel.confirm(Entry::Reload));
    }

    /// Regression: stepping Scaling blanked the input map's value.
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
            .position(|row| row.label() == "Scaling")
            .expect("scaling is a quick row in the mode");
        assert_eq!(panel.value(at), "Off");
    }
}
