//! The edge strips: Quick Access, the actions at the right edge, and Quick
//! Menu, the places to go at the left. Each has one table per thing it opens
//! over (a page, a game, the start page), so what one hides from another is
//! data rather than branches. Over the start page Quick Access leads with the
//! focused bookmark's rows, headed by its name, before the page's own.
//! Quick rows are [`crate::overlay::settings`] rows flagged `quick`: the same
//! label, values and config field, stepped live here instead of on close. The
//! central router ([`crate::app`]) drives it; [`crate::ui`] renders it.

use crate::browser::TabMode;
use crate::config::AppConfig;
use crate::event::bindings::Action;
use crate::overlay::menu::Section;
use crate::overlay::settings::{self, Kind, Settings};

/// Which strip is up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strip {
    /// Actions on the page, the game or the start page, at the right edge.
    QuickAccess,
    /// Places to go, at the left edge.
    QuickMenu,
}

impl Strip {
    pub fn name(self) -> &'static str {
        match self {
            Strip::QuickAccess => "Quick Access",
            Strip::QuickMenu => "Quick Menu",
        }
    }
}

/// A row of a bookmark's options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionsRow {
    Open,
    /// Step how it opens.
    LaunchAs,
    /// Whether it opens in reader view; not for a game.
    Reader,
    Rename,
    Tags,
    /// One place earlier in the manual order.
    MoveLeft,
    /// One place later in the manual order.
    MoveRight,
    Remove,
}

/// What the keyboard types for a bookmark's options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionsText {
    Name,
    /// Comma-separated.
    Tags,
}

impl OptionsText {
    /// The row that asks for it.
    pub fn row(self) -> OptionsRow {
        match self {
            OptionsText::Name => OptionsRow::Rename,
            OptionsText::Tags => OptionsRow::Tags,
        }
    }
}

/// The start-page tile whose rows lead Quick Access.
pub struct HomeTile {
    pub id: String,
    pub title: String,
    /// Where it leads: the name alone says nothing once renamed.
    pub site: String,
    /// Whether the grid is in the manual order, which alone the moves act on.
    pub manual: bool,
    /// Whether it opens as a game, which has no reader view.
    pub game: bool,
}

/// The bookmark the options rows act on.
struct OptionsBookmark {
    id: String,
    /// The text being typed, and for what.
    naming: Option<(OptionsText, String)>,
}

/// What a strip opens over: a tab in its mode, or the start page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Over {
    Tab(TabMode),
    Home,
}

/// Where a quick row shows: over a browser tab (a page or its reader view),
/// over a game, or over the start page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modes {
    Browser,
    Game,
    Home,
}

impl Modes {
    pub fn includes(self, over: Over) -> bool {
        match self {
            Modes::Browser => matches!(over, Over::Tab(mode) if mode != TabMode::Game),
            Modes::Game => over == Over::Tab(TabMode::Game),
            Modes::Home => over == Over::Home,
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
    /// Stop running the tab's bookmark and go back to the start page.
    Leave,
    /// Close retsurf, on a second press (see [`QuickAccess::confirm`]).
    Quit,
    /// A row of a bookmark's options.
    Options(OptionsRow),
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
    Slot::Row(Entry::Run(Action::Bookmark)),
    Slot::Row(Entry::Exit),
];

/// Quick Access over the start page: how it shows its bookmarks.
const ACCESS_HOME: &[Slot] = &[Slot::QuickRows];
/// What heads the page's own rows when a tile's rows come first.
const HOME_HEAD: &str = "Home";

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

/// A bookmark's options; the moves show only where the order is the manual one.
const OPTIONS: &[Slot] = &[
    Slot::Row(Entry::Options(OptionsRow::Open)),
    Slot::Row(Entry::Options(OptionsRow::LaunchAs)),
    Slot::Row(Entry::Options(OptionsRow::Reader)),
    Slot::Row(Entry::Options(OptionsRow::Rename)),
    Slot::Row(Entry::Options(OptionsRow::Tags)),
    Slot::Row(Entry::Options(OptionsRow::MoveLeft)),
    Slot::Row(Entry::Options(OptionsRow::MoveRight)),
    Slot::Row(Entry::Options(OptionsRow::Remove)),
];

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
            Entry::Leave => "Exit",
            Entry::Quit => "Quit retsurf",
            Entry::Options(OptionsRow::Open) => "Open",
            Entry::Options(OptionsRow::LaunchAs) => "Launch as",
            Entry::Options(OptionsRow::Reader) => "Reader view",
            Entry::Options(OptionsRow::Rename) => "Rename...",
            Entry::Options(OptionsRow::Tags) => "Tags...",
            Entry::Options(OptionsRow::MoveLeft) => "Move left",
            Entry::Options(OptionsRow::MoveRight) => "Move right",
            Entry::Options(OptionsRow::Remove) => "Remove",
        }
    }

    /// What the row shows once armed, for a row that acts on a second press
    /// (see [`QuickAccess::confirm`]); `None` for a row that acts at once.
    pub fn confirm_hint(self) -> Option<&'static str> {
        match self {
            Entry::Quit => Some("press again to quit"),
            Entry::Reload => Some("press again to reload"),
            Entry::Options(OptionsRow::Remove) => Some("press again to remove"),
            _ => None,
        }
    }

    /// Whether Left/Right step the row's value.
    pub fn steps(self) -> bool {
        matches!(
            self,
            Entry::Quick(_)
                | Entry::InputMap
                | Entry::Run(Action::Reader)
                | Entry::Options(OptionsRow::LaunchAs | OptionsRow::Reader)
        )
    }
}

/// The entries `strip` shows `over` a tab or the start page, in its table's
/// order. Over a launched bookmark Quick Access ends in the way out of it,
/// which in a game stands in for leaving Game Mode alone.
fn entries(strip: Strip, over: Over, launched: bool) -> Vec<Entry> {
    let layout = match (strip, over) {
        (Strip::QuickAccess, Over::Home) => ACCESS_HOME,
        (Strip::QuickAccess, Over::Tab(TabMode::Game)) => ACCESS_GAME,
        (Strip::QuickAccess, Over::Tab(_)) => ACCESS_BROWSER,
        (Strip::QuickMenu, Over::Tab(TabMode::Game)) => MENU_GAME,
        (Strip::QuickMenu, _) => MENU_BROWSER,
    };
    let quick = |i: usize| {
        let field = &Settings::fields()[i];
        field.shown() && field.quick.is_some_and(|modes| modes.includes(over))
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
    if launched && strip == Strip::QuickAccess {
        match rows.iter().position(|row| *row == Entry::Exit) {
            Some(at) => rows[at] = Entry::Leave,
            None => rows.push(Entry::Leave),
        }
    }
    rows
}

/// A bookmark's options rows: `manual` offers the moves, which mean nothing in
/// another order, and `game` drops the reader view, which a game cannot open in.
fn options_rows(manual: bool, game: bool) -> Vec<Entry> {
    OPTIONS
        .iter()
        .filter_map(|slot| match slot {
            Slot::Row(entry) => Some(*entry),
            Slot::Quick(_) | Slot::QuickRows => None,
        })
        .filter(|row| match row {
            Entry::Options(OptionsRow::MoveLeft | OptionsRow::MoveRight) => manual,
            Entry::Options(OptionsRow::Reader) => !game,
            _ => true,
        })
        .collect()
}

pub struct QuickAccess {
    pub visible: bool,
    strip: Strip,
    rows: Vec<Entry>,
    /// Each row's value as last read from the config (empty for rows with none).
    values: Vec<String>,
    /// Each on/off row's state, drawn as a switch in place of its value.
    flags: Vec<Option<bool>>,
    selected: usize,
    /// A two-press row pressed once; any move disarms it.
    armed: Option<Entry>,
    /// The bookmark the options rows are open on.
    bookmark: Option<OptionsBookmark>,
    /// Section heads: each drawn over the row at its index.
    heads: Vec<(usize, String)>,
}

impl QuickAccess {
    pub fn new() -> Self {
        Self {
            visible: false,
            strip: Strip::QuickAccess,
            rows: Vec::new(),
            values: Vec::new(),
            flags: Vec::new(),
            selected: 0,
            armed: None,
            bookmark: None,
            heads: Vec::new(),
        }
    }

    /// Show `strip`'s rows `over` what is on screen, highlighting the first;
    /// `launched`: whether the tab runs a bookmark.
    pub fn open(&mut self, strip: Strip, over: Over, launched: bool, config: &AppConfig) {
        self.show(strip, entries(strip, over, launched));
        self.refresh(config);
        if let Some(at) = self.position(Entry::Run(Action::Reader)) {
            self.flags[at] = Some(over == Over::Tab(TabMode::Reader));
        }
    }

    /// Show `rows` as `strip`, highlighting the first, with no values yet.
    fn show(&mut self, strip: Strip, rows: Vec<Entry>) {
        self.visible = true;
        self.strip = strip;
        self.bookmark = None;
        self.heads.clear();
        self.selected = 0;
        self.armed = None;
        self.values = vec![String::new(); rows.len()];
        self.flags = vec![None; rows.len()];
        self.rows = rows;
    }

    /// Show Quick Access over the start page: `tile`'s rows, headed by its
    /// name, then the page's own rows headed as the page.
    pub fn open_home(&mut self, tile: Option<HomeTile>, launched: bool, config: &AppConfig) {
        let mut rows = Vec::new();
        let mut heads = Vec::new();
        if let Some(tile) = &tile {
            heads.push((0, tile.title.clone()));
            rows.extend(options_rows(tile.manual, tile.game));
            heads.push((rows.len(), HOME_HEAD.to_string()));
        }
        rows.extend(entries(Strip::QuickAccess, Over::Home, launched));
        self.show(Strip::QuickAccess, rows);
        self.refresh(config);
        self.heads = heads;
        if let Some(tile) = tile {
            self.set_value(Entry::Options(OptionsRow::Open), tile.site);
            self.bookmark = Some(OptionsBookmark {
                id: tile.id,
                naming: None,
            });
        }
    }

    /// The section heads, each over the row at its index.
    pub fn heads(&self) -> &[(usize, String)] {
        &self.heads
    }

    /// Show `entry` as a switch in state `on`.
    pub fn set_flag(&mut self, entry: Entry, on: bool) {
        if let Some(at) = self.position(entry) {
            self.flags[at] = Some(on);
        }
    }

    /// Focus `entry`'s row, when the strip has one.
    pub fn select_entry(&mut self, entry: Entry) {
        if let Some(at) = self.position(entry) {
            self.select(at);
        }
    }

    /// Show `value` beside `entry`, for a row whose state the config does not
    /// hold (a page's bookmark, the tab count).
    pub fn set_value(&mut self, entry: Entry, value: String) {
        if let Some(at) = self.position(entry) {
            self.values[at] = value;
        }
    }

    fn position(&self, entry: Entry) -> Option<usize> {
        self.rows.iter().position(|row| *row == entry)
    }

    /// Re-read the quick rows' values from `config`; values from
    /// [`Self::set_value`] stay.
    pub fn refresh(&mut self, config: &AppConfig) {
        for (at, row) in self.rows.iter().enumerate() {
            if let Entry::Quick(i) = *row {
                let kind = &Settings::fields()[i].kind;
                self.values[at] = settings::value_of(kind, config);
                if let Kind::Bool { get, .. } = kind {
                    self.flags[at] = Some(get(config));
                }
            }
        }
    }

    /// The bookmark the options rows are open on.
    pub fn bookmark_id(&self) -> Option<&str> {
        self.bookmark.as_ref().map(|b| b.id.as_str())
    }

    /// Rename the bookmark's section.
    pub fn set_title(&mut self, title: String) {
        if let Some((0, head)) = self.heads.first_mut() {
            *head = title;
        }
    }

    /// What the strip's header reads.
    pub fn title(&self) -> &str {
        self.strip.name()
    }

    /// Hand the keyboard `text` to edit as `what`.
    pub fn start_naming(&mut self, what: OptionsText, text: String) {
        if let Some(bookmark) = &mut self.bookmark {
            bookmark.naming = Some((what, text));
        }
    }

    /// What the keyboard is typing for, while it is.
    pub fn naming(&self) -> Option<OptionsText> {
        self.bookmark
            .as_ref()?
            .naming
            .as_ref()
            .map(|(what, _)| *what)
    }

    pub fn naming_text_mut(&mut self) -> Option<&mut String> {
        self.bookmark
            .as_mut()?
            .naming
            .as_mut()
            .map(|(_, text)| text)
    }

    /// End the typing, with what it was for.
    pub fn take_naming(&mut self) -> Option<OptionsText> {
        self.bookmark.as_mut()?.naming.take().map(|(what, _)| what)
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.bookmark = None;
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
        if let Some(typed) = self.typed(index) {
            return typed;
        }
        self.values.get(index).map_or("", String::as_str)
    }

    /// What the keyboard is typing, when it types for the row at `index`.
    fn typed(&self, index: usize) -> Option<&str> {
        let (what, text) = self.bookmark.as_ref()?.naming.as_ref()?;
        let row = Entry::Options(what.row());
        (self.rows.get(index) == Some(&row)).then_some(text.as_str())
    }

    /// The row's on/off state, or `None` for a row with a text value.
    pub fn flag(&self, index: usize) -> Option<bool> {
        self.flags.get(index).copied().flatten()
    }

    pub fn row(&self) -> Entry {
        self.rows[self.selected]
    }

    #[cfg(test)]
    fn value_count(&self) -> usize {
        self.values.len().min(self.flags.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(mode: TabMode) -> QuickAccess {
        let mut panel = QuickAccess::new();
        panel.open(
            Strip::QuickAccess,
            Over::Tab(mode),
            false,
            &AppConfig::default(),
        );
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
        let game = entries(Strip::QuickAccess, Over::Tab(TabMode::Game), false);
        let browser = entries(Strip::QuickAccess, Over::Tab(TabMode::Page), false);
        assert!(game.contains(&Entry::Exit) && !game.contains(&Entry::Enter));
        assert!(browser.contains(&Entry::Enter) && !browser.contains(&Entry::Exit));
        assert!(!browser.contains(&Entry::Osk));
        let has = |rows: &[Entry], label| rows.iter().any(|row| row.label() == label);
        assert!(has(&game, "Scaling") && !has(&browser, "Scaling"));
        assert!(has(&browser, "Page theme") && !has(&game, "Page theme"));
        // A game keeps its page, so only bookmarking it runs from its strip.
        assert!(!game.iter().any(|row| {
            matches!(row, Entry::List(_))
                || matches!(row, Entry::Run(action) if *action != Action::Bookmark)
        }));
        // Actions in Quick Access, places to go in Quick Menu.
        let door = |row: &Entry| {
            matches!(
                row,
                Entry::List(_) | Entry::Settings | Entry::Quit | Entry::Run(Action::Home)
            )
        };
        assert!(!browser.iter().any(door));
        let menu = entries(Strip::QuickMenu, Over::Tab(TabMode::Page), false);
        assert!(menu.iter().all(door));
        let bookmark = options_rows(true, false);
        assert!(bookmark.iter().all(|row| matches!(row, Entry::Options(_))));
        assert_eq!(
            entries(Strip::QuickMenu, Over::Tab(TabMode::Game), false),
            [Entry::Settings, Entry::Quit]
        );
        assert!(!game.contains(&Entry::Settings));
        for row in game.iter().chain(&browser).chain(&menu) {
            assert!(!row.label().ends_with("..."), "{row:?}");
        }
    }

    /// The start page has no page to act on: its strip is its own settings.
    #[test]
    fn the_start_page_lists_only_its_quick_rows() {
        let home = entries(Strip::QuickAccess, Over::Home, false);
        assert!(home.iter().all(|row| matches!(row, Entry::Quick(_))));
        assert!(home.iter().any(|row| row.label() == "Sort"));
        assert!(!home.iter().any(|row| row.label() == "Page theme"));
        assert_eq!(
            entries(Strip::QuickMenu, Over::Home, false),
            entries(Strip::QuickMenu, Over::Tab(TabMode::Page), false)
        );
    }

    #[test]
    fn the_reader_row_shows_whether_the_tab_is_in_it() {
        let reader_row = |panel: &QuickAccess| {
            let at = panel
                .rows()
                .iter()
                .position(|row| *row == Entry::Run(Action::Reader))
                .expect("the browser rows carry the reader view");
            (panel.label(at), panel.flag(at))
        };
        assert_eq!(
            reader_row(&open(TabMode::Page)),
            ("Reader view", Some(false))
        );
        let panel = open(TabMode::Reader);
        assert_eq!(reader_row(&panel), ("Reader view", Some(true)));
        assert_eq!(panel.rows(), open(TabMode::Page).rows());
    }

    #[test]
    fn a_launched_bookmark_ends_quick_access_in_its_way_out() {
        let game = entries(Strip::QuickAccess, Over::Tab(TabMode::Game), true);
        assert!(game.contains(&Entry::Leave) && !game.contains(&Entry::Exit));
        let app = entries(Strip::QuickAccess, Over::Tab(TabMode::Page), true);
        assert_eq!(app.last(), Some(&Entry::Leave));
        let menu = entries(Strip::QuickMenu, Over::Tab(TabMode::Page), true);
        assert!(!menu.contains(&Entry::Leave));
    }

    fn tile(manual: bool, game: bool) -> HomeTile {
        HomeTile {
            id: "a".into(),
            title: "A".into(),
            site: "a.example".into(),
            manual,
            game,
        }
    }

    /// Over the start page a focused tile's rows lead, each section headed,
    /// and the page's rows close.
    #[test]
    fn a_tile_leads_quick_access_over_the_start_page() {
        let mut panel = QuickAccess::new();
        panel.open_home(Some(tile(true, false)), false, &AppConfig::default());
        let options = options_rows(true, false);
        assert_eq!(&panel.rows()[..options.len()], &options[..]);
        assert!(panel.rows()[options.len()..]
            .iter()
            .all(|row| matches!(row, Entry::Quick(_))));
        assert_eq!(
            panel.heads(),
            [(0, "A".to_string()), (options.len(), HOME_HEAD.to_string())]
        );
        assert_eq!(panel.value(0), "a.example");
        panel.set_title("B".into());
        assert_eq!(panel.heads()[0].1, "B");
        panel.open_home(None, false, &AppConfig::default());
        assert!(panel.heads().is_empty());
        assert!(panel
            .rows()
            .iter()
            .all(|row| matches!(row, Entry::Quick(_))));
    }

    #[test]
    fn the_options_move_only_in_the_manual_order() {
        let mut panel = QuickAccess::new();
        let moves =
            |panel: &QuickAccess| panel.rows().contains(&Entry::Options(OptionsRow::MoveLeft));
        panel.open_home(Some(tile(true, false)), false, &AppConfig::default());
        assert!(moves(&panel) && panel.bookmark_id() == Some("a"));
        panel.open_home(Some(tile(false, true)), false, &AppConfig::default());
        assert!(!moves(&panel));
        assert!(!panel.rows().contains(&Entry::Options(OptionsRow::Reader)));
        assert_eq!(panel.rows().len(), panel.value_count());
        let rename = panel
            .rows()
            .iter()
            .position(|row| *row == Entry::Options(OptionsRow::Rename))
            .expect("every options strip renames");
        panel.start_naming(OptionsText::Name, "Typed".into());
        assert_eq!(panel.value(rename), "Typed");
        panel.take_naming();
        assert_eq!(panel.value(rename), "");
        panel.close();
        assert_eq!(panel.bookmark_id(), None);
    }

    #[test]
    fn quitting_takes_a_second_press() {
        let mut panel = QuickAccess::new();
        panel.open(
            Strip::QuickMenu,
            Over::Tab(TabMode::Page),
            false,
            &AppConfig::default(),
        );
        panel.move_sel(-1);
        assert_eq!(panel.row(), Entry::Quit);
        assert!(!panel.confirm(Entry::Quit));
        assert_eq!(panel.value(panel.selected()), "press again to quit");
        panel.move_sel(-1);
        panel.move_sel(1);
        assert!(!panel.confirm(Entry::Quit));
        assert!(panel.confirm(Entry::Quit));
    }

    /// A config bool shows as a switch, a choice keeps its text.
    #[test]
    fn on_off_rows_carry_a_flag_and_choices_do_not() {
        let panel = open(TabMode::Page);
        let flag_of = |label| {
            let at = (0..panel.rows().len()).find(|&i| panel.label(i) == label);
            panel.flag(at.expect("a browser quick row"))
        };
        assert_eq!(
            flag_of("Ad blocker"),
            Some(AppConfig::default().adblock.enabled)
        );
        assert_eq!(flag_of("User agent"), None);
        assert_eq!(flag_of("Bookmark"), None);
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
        assert_eq!(panel.value(at), "None");
    }
}
