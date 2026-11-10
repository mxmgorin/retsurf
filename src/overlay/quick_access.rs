//! The edge strips: Quick Access, the actions at the right edge, and Quick
//! Menu, the places to go at the left. Each has one table per thing it opens
//! over (a page, a game, the start page), so what one hides from another is
//! data rather than branches. Over a tab Quick Access runs what acts on it,
//! then what sets it, then the way out, so a row keeps its place whatever
//! applies; where a bookmark's site settings do, its reader view stands in for
//! the tab's and its launch and reset rows join the settings. Over the start
//! page the focused bookmark's rows lead, before the page's own. The header
//! names what the rows act on.
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
    /// Whether its site's articles open in reader view; not for a game.
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

/// The bookmark whose settings apply to the tab a strip opens over.
#[derive(Clone, Copy)]
pub struct SiteHead<'a> {
    pub id: &'a str,
    /// Its shown name; empty for one since removed.
    pub name: &'a str,
    /// The global values its own stand in for.
    pub global: &'a AppConfig,
    /// How many settings it holds as its own: what Reset drops.
    pub overrides: usize,
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
    /// Over a tab in any mode.
    Tab,
    Home,
}

impl Modes {
    pub fn includes(self, over: Over) -> bool {
        match self {
            Modes::Browser => matches!(over, Over::Tab(mode) if mode != TabMode::Game),
            Modes::Game => over == Over::Tab(TabMode::Game),
            Modes::Tab => matches!(over, Over::Tab(_)),
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
    /// Reload the page; over a game, on a second press.
    Reload,
    /// Turn Game Mode off.
    Exit,
    /// Stop running the tab's bookmark and go back to the start page.
    Leave,
    /// Drop the shown site's setting overrides, on a second press.
    ResetSite,
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

/// A strip's table: runs of slots, a rule drawn between one and the next.
type Layout = &'static [&'static [Slot]];

/// Quick Access in the browser: what acts on the page, then what sets it. B
/// closes a strip, so no row goes back.
const ACCESS_BROWSER: Layout = &[
    &[
        Slot::Row(Entry::Reload),
        Slot::Row(Entry::Enter),
        Slot::Row(Entry::Run(Action::Bookmark)),
    ],
    &[
        Slot::QuickRows,
        Slot::Row(Entry::Run(Action::Reader)),
        Slot::Quick("Page theme"),
    ],
];

/// Quick Access over a game: what acts on it, then what sets it.
const ACCESS_GAME: Layout = &[
    &[
        Slot::Row(Entry::Reload),
        Slot::Row(Entry::Osk),
        Slot::Row(Entry::Run(Action::Bookmark)),
    ],
    &[Slot::QuickRows, Slot::Row(Entry::InputMap)],
];

/// Quick Access over the start page: how it shows its bookmarks.
const ACCESS_HOME: Layout = &[&[Slot::QuickRows]];
/// Put before the JS engine row's value while A would reopen the page.
const REOPEN_HINT: &str = "Reopen: ";

/// What heads the page's own rows when a tile's rows come first, and the
/// header without one.
const HOME_HEAD: &str = "Home";

/// Quick Menu in the browser: where to go, so home leads.
const MENU_BROWSER: Layout = &[&[
    Slot::Row(Entry::Run(Action::Home)),
    Slot::Row(Entry::List(Section::Tabs)),
    Slot::Row(Entry::List(Section::Bookmarks)),
    Slot::Row(Entry::List(Section::History)),
    Slot::Row(Entry::List(Section::Downloads)),
    Slot::Row(Entry::Settings),
    Slot::Row(Entry::Quit),
]];

/// Quick Menu over a game: only what leaves the game where it is.
const MENU_GAME: Layout = &[&[Slot::Row(Entry::Settings), Slot::Row(Entry::Quit)]];

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
            Entry::Leave => "Close",
            Entry::ResetSite => "Reset site settings",
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
            Entry::ResetSite => Some("press again to reset"),
            Entry::Run(Action::Bookmark) | Entry::Options(OptionsRow::Remove) => {
                Some("press again to remove")
            }
            _ => None,
        }
    }

    /// Whether A opens a screen of the row's own.
    pub fn opens(self) -> bool {
        matches!(self, Entry::InputMap | Entry::List(_) | Entry::Settings)
    }

    /// Whether the row takes away what cannot be got back.
    pub fn destructive(self) -> bool {
        matches!(
            self,
            Entry::ResetSite | Entry::Quit | Entry::Options(OptionsRow::Remove)
        )
    }

    /// What A does on the row, unarmed.
    fn press(self) -> Press {
        match self {
            _ if self.opens() => Press::Open,
            Entry::Quick(i) if matches!(Settings::fields()[i].kind, Kind::Bool { .. }) => {
                Press::Switch
            }
            Entry::Run(Action::Reader) | Entry::Options(OptionsRow::Reader) => Press::Switch,
            Entry::Quick(_) | Entry::Options(OptionsRow::LaunchAs) => Press::Next,
            Entry::Options(OptionsRow::Rename | OptionsRow::Tags) => Press::Edit,
            _ => Press::Select,
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

/// What A does on a row, as the strip's footer names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// Does what the row's label says.
    Select,
    /// Opens a screen of the row's own.
    Open,
    /// Flips the row's switch.
    Switch,
    /// Steps the row's value to the next.
    Next,
    /// Hands the keyboard the row's text.
    Edit,
    /// Reopens the page under the value the row shows.
    Reopen,
    /// Acts on a row pressed once already.
    Confirm,
}

impl Press {
    pub fn verb(self) -> &'static str {
        match self {
            Press::Select => "select",
            Press::Open => "open",
            Press::Switch => "switch",
            Press::Next => "next",
            Press::Edit => "edit",
            Press::Reopen => "reopen",
            Press::Confirm => "confirm",
        }
    }
}

/// The runs of entries `strip` shows `over` a tab or the start page, in its
/// table's order, without the way out (see [`way_out`]).
fn groups(strip: Strip, over: Over) -> Vec<Vec<Entry>> {
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
            .flat_map(|group| group.iter())
            .any(|slot| matches!(slot, Slot::Quick(l) if *l == label))
    };
    let expand = |group: &[Slot]| {
        let mut rows = Vec::new();
        for &slot in group {
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
    };
    layout.iter().map(|group| expand(group)).collect()
}

/// The row that ends Quick Access, if any: leaving a launched bookmark, else
/// in a game leaving Game Mode.
fn way_out(strip: Strip, over: Over, launched: bool) -> Option<Entry> {
    match (strip, over, launched) {
        (Strip::QuickMenu, ..) => None,
        (_, _, true) => Some(Entry::Leave),
        (_, Over::Tab(TabMode::Game), false) => Some(Entry::Exit),
        _ => None,
    }
}

/// What the header reads over a tab: in Quick Access its site's bookmark
/// name, else `page`, else the strip's name; in Quick Menu the strip's name.
fn header(strip: Strip, site: Option<SiteHead>, page: &str) -> String {
    let named = site.map(|site| site.name).filter(|name| !name.is_empty());
    match strip {
        Strip::QuickAccess => named
            .or(Some(page).filter(|page| !page.is_empty()))
            .unwrap_or(strip.name()),
        Strip::QuickMenu => strip.name(),
    }
    .to_string()
}

/// [`Entry::Leave`]'s label for the bookmark named `launched`.
fn leave_label(launched: Option<&str>) -> String {
    let close = Entry::Leave.label();
    match launched {
        Some(name) if !name.is_empty() => format!("{close} {name}"),
        _ => close.to_string(),
    }
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

/// What parts one run of rows from the next.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Break {
    /// A line.
    Rule,
    /// A head naming the rows under it.
    Head(String),
}

pub struct QuickAccess {
    pub visible: bool,
    strip: Strip,
    rows: Vec<Entry>,
    /// Each row's value as last read from the config (empty for rows with none).
    values: Vec<String>,
    /// Each on/off row's state, drawn as a switch in place of its value.
    flags: Vec<Option<bool>>,
    /// Whether each row holds a value of the shown site's own.
    own: Vec<bool>,
    /// The global values a site's own stand in for, while one applies.
    global: Option<AppConfig>,
    selected: usize,
    /// A two-press row pressed once; any move disarms it.
    armed: Option<Entry>,
    /// The bookmark the options rows are open on.
    bookmark: Option<OptionsBookmark>,
    /// What the header reads: what the rows act on.
    title: String,
    /// What parts the runs of rows, each drawn over the row at its index.
    breaks: Vec<(usize, Break)>,
    /// [`Entry::Leave`]'s label, naming the bookmark it closes.
    leave: String,
    /// [`Entry::ResetSite`]'s label, counting what it drops.
    reset: String,
    /// The JS preset the page opened with, while one is known.
    page_js: Option<String>,
    /// The row whose A reopens the page, while one would.
    reopen: Option<usize>,
}

impl QuickAccess {
    pub fn new() -> Self {
        Self {
            visible: false,
            strip: Strip::QuickAccess,
            rows: Vec::new(),
            values: Vec::new(),
            flags: Vec::new(),
            own: Vec::new(),
            global: None,
            selected: 0,
            armed: None,
            bookmark: None,
            title: String::new(),
            breaks: Vec::new(),
            leave: String::new(),
            reset: String::new(),
            page_js: None,
            reopen: None,
        }
    }

    /// Show `strip`'s rows `over` what is on screen, highlighting the first.
    /// `site`: the bookmark whose settings apply, if any; `launched`: whether
    /// the tab runs it; `page`: the shown page's site, for the header.
    pub fn open(
        &mut self,
        strip: Strip,
        over: Over,
        site: Option<SiteHead>,
        launched: bool,
        page: &str,
        config: &AppConfig,
    ) {
        let mut groups = groups(strip, over);
        let site = site.filter(|_| strip == Strip::QuickAccess && over != Over::Home);
        if let (Some(site), Some(settings)) = (site, groups.last_mut()) {
            // The site's reader view stands in for the tab's, in its place.
            for row in settings.iter_mut() {
                if *row == Entry::Run(Action::Reader) {
                    *row = Entry::Options(OptionsRow::Reader);
                }
            }
            settings.push(Entry::Options(OptionsRow::LaunchAs));
            if site.overrides > 0 {
                settings.push(Entry::ResetSite);
            }
        }
        if let Some(last) = groups.last_mut() {
            last.extend(way_out(strip, over, launched));
        }
        let mut rows = Vec::new();
        let mut breaks = Vec::new();
        for group in groups.into_iter().filter(|group| !group.is_empty()) {
            if !rows.is_empty() {
                breaks.push((rows.len(), Break::Rule));
            }
            rows.extend(group);
        }
        self.show(strip, rows);
        self.breaks = breaks;
        self.title = header(strip, site, page);
        self.global = site.map(|site| site.global.clone());
        self.set_overrides(site.map_or(0, |site| site.overrides));
        self.leave = leave_label(site.filter(|_| launched).map(|site| site.name));
        if let Some(site) = site.filter(|_| strip == Strip::QuickAccess) {
            self.bookmark = Some(OptionsBookmark {
                id: site.id.to_string(),
                naming: None,
            });
        }
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
        self.page_js = None;
        self.reopen = None;
        self.breaks.clear();
        self.selected = 0;
        self.armed = None;
        self.values = vec![String::new(); rows.len()];
        self.flags = vec![None; rows.len()];
        self.own = vec![false; rows.len()];
        self.global = None;
        self.rows = rows;
    }

    /// Show Quick Access over the start page: `tile`'s rows under its name in
    /// the header, then the page's own rows headed as the page.
    pub fn open_home(
        &mut self,
        tile: Option<HomeTile>,
        launched: Option<&str>,
        config: &AppConfig,
    ) {
        let mut rows = Vec::new();
        let mut breaks = Vec::new();
        if let Some(tile) = &tile {
            rows.extend(options_rows(tile.manual, tile.game));
            breaks.push((rows.len(), Break::Head(HOME_HEAD.to_string())));
        }
        rows.extend(groups(Strip::QuickAccess, Over::Home).concat());
        rows.extend(way_out(Strip::QuickAccess, Over::Home, launched.is_some()));
        self.show(Strip::QuickAccess, rows);
        self.leave = leave_label(launched);
        self.refresh(config);
        self.breaks = breaks;
        self.title = tile
            .as_ref()
            .map_or_else(|| HOME_HEAD.to_string(), |tile| tile.title.clone());
        if let Some(tile) = tile {
            self.set_value(Entry::Options(OptionsRow::Open), tile.site);
            self.bookmark = Some(OptionsBookmark {
                id: tile.id,
                naming: None,
            });
        }
    }

    /// What parts the runs of rows, each over the row at its index.
    pub fn breaks(&self) -> &[(usize, Break)] {
        &self.breaks
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
        self.reopen = None;
        for (at, row) in self.rows.iter().enumerate() {
            let global = self.global.as_ref();
            self.own[at] = match *row {
                Entry::Quick(i) => global
                    .is_some_and(|g| settings::differs(&Settings::fields()[i].kind, config, g)),
                Entry::InputMap => {
                    global.is_some_and(|g| g.game_mode.input_map != config.game_mode.input_map)
                }
                _ => false,
            };
            if let Entry::Quick(i) = *row {
                let kind = &Settings::fields()[i].kind;
                self.values[at] = settings::value_of(kind, config);
                if let Kind::Bool { get, .. } = kind {
                    self.flags[at] = Some(get(config));
                }
                if self.wants_reopen(*row, config) {
                    self.values[at].insert_str(0, REOPEN_HINT);
                    self.reopen = Some(at);
                }
            }
        }
    }

    /// Count `n` settings the shown site holds as its own in the Reset row.
    pub fn set_overrides(&mut self, n: usize) {
        let reset = Entry::ResetSite.label();
        self.reset = match n {
            0 => reset.to_string(),
            1 => "Reset 1 site setting".to_string(),
            n => format!("Reset {n} site settings"),
        };
    }

    /// Whether the row at `index` holds the shown site's own value, which X
    /// sets back to the global one; the site's reader view counts while on.
    pub fn own(&self, index: usize) -> bool {
        match self.rows.get(index) {
            Some(Entry::Options(OptionsRow::Reader)) => {
                self.global.is_some() && self.flag(index) == Some(true)
            }
            Some(_) => self.own[index],
            None => false,
        }
    }

    /// Tell the rows which JS preset the page opened with, `None` where
    /// none applies.
    pub fn set_page_js(&mut self, preset: Option<String>) {
        self.page_js = preset;
    }

    /// Whether `row` is the JS engine row set to another preset than the
    /// page opened with, which A then reopens it in.
    pub fn wants_reopen(&self, row: Entry, config: &AppConfig) -> bool {
        let Entry::Quick(i) = row else {
            return false;
        };
        Settings::fields()[i].label == settings::JS_ENGINE
            && self
                .page_js
                .as_deref()
                .is_some_and(|page| page != config.performance.js_preset)
    }

    /// The bookmark the options rows are open on.
    pub fn bookmark_id(&self) -> Option<&str> {
        self.bookmark.as_ref().map(|b| b.id.as_str())
    }

    /// Rename what the rows act on.
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// What the strip's header reads.
    pub fn title(&self) -> &str {
        &self.title
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

    /// A on a two-press row: arms it, or disarms it and returns `true` when
    /// it already was.
    pub fn confirm(&mut self, row: Entry) -> bool {
        let confirmed = self.armed == Some(row);
        self.armed = (!confirmed).then_some(row);
        confirmed
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn rows(&self) -> &[Entry] {
        &self.rows
    }

    pub fn label(&self, index: usize) -> &str {
        match self.rows.get(index) {
            Some(Entry::Leave) => &self.leave,
            Some(Entry::ResetSite) => &self.reset,
            Some(row) => row.label(),
            None => "",
        }
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

    /// What A does on the focused row.
    pub fn press(&self) -> Press {
        let row = self.row();
        match row {
            _ if self.armed == Some(row) && row.confirm_hint().is_some() => Press::Confirm,
            _ if self.reopen == Some(self.selected) => Press::Reopen,
            _ => row.press(),
        }
    }

    #[cfg(test)]
    fn value_count(&self) -> usize {
        self.values.len().min(self.flags.len()).min(self.own.len())
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
            None,
            false,
            "",
            &AppConfig::default(),
        );
        panel
    }

    /// The rows `strip` shows `over` what is on screen, no site's bookmark
    /// applying.
    fn entries(strip: Strip, over: Over, launched: bool) -> Vec<Entry> {
        let mut panel = QuickAccess::new();
        panel.open(strip, over, None, launched, "", &AppConfig::default());
        panel.rows().to_vec()
    }

    #[test]
    fn the_highlight_starts_on_the_first_row_and_wraps_at_the_ends() {
        let mut panel = open(TabMode::Game);
        assert_eq!(panel.row(), Entry::Reload);
        assert_eq!(panel.label(1), "On-screen keyboard");
        panel.move_sel(-1);
        assert_eq!(panel.row(), Entry::Exit);
        panel.move_sel(1);
        assert_eq!(panel.row(), Entry::Reload);
        let mut panel = open(TabMode::Page);
        assert_eq!(panel.row(), Entry::Reload);
        let labels: Vec<_> = (0..panel.rows().len()).map(|i| panel.label(i)).collect();
        assert_eq!(
            labels,
            [
                "Reload",
                "Enter game mode",
                "Bookmark",
                "User agent",
                "Ad blocker",
                "JS engine",
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
        fn reader_row(panel: &QuickAccess) -> (&str, Option<bool>) {
            let at = panel
                .rows()
                .iter()
                .position(|row| *row == Entry::Run(Action::Reader))
                .expect("the browser rows carry the reader view");
            (panel.label(at), panel.flag(at))
        }
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

    /// The global values the sites below stand in for.
    static GLOBAL: std::sync::LazyLock<AppConfig> = std::sync::LazyLock::new(AppConfig::default);

    fn itch() -> SiteHead<'static> {
        SiteHead {
            id: "itch",
            name: "itch.io",
            global: &GLOBAL,
            overrides: 1,
        }
    }

    /// A row holding the site's own value is marked; Reset counts what it
    /// drops and shows only with something to drop.
    #[test]
    fn a_sites_own_values_are_marked_and_counted() {
        let global = AppConfig::default();
        let mut config = global.clone();
        config.browser.user_agent = "mobile".to_string();
        let over = Over::Tab(TabMode::Page);
        let at = |panel: &QuickAccess, label: &str| {
            (0..panel.rows().len())
                .find(|&i| panel.label(i) == label)
                .expect("a site's page row")
        };
        let mut panel = QuickAccess::new();
        let site = SiteHead {
            global: &global,
            overrides: 2,
            ..itch()
        };
        panel.open(Strip::QuickAccess, over, Some(site), false, "", &config);
        assert!(panel.own(at(&panel, "User agent")));
        assert!(!panel.own(at(&panel, "Ad blocker")));
        let reset = at(&panel, "Reset 2 site settings");
        panel.set_overrides(1);
        assert_eq!(panel.label(reset), "Reset 1 site setting");
        panel.refresh(&global);
        assert!(!panel.own(at(&panel, "User agent")));
        let clean = SiteHead {
            overrides: 0,
            ..site
        };
        panel.open(Strip::QuickAccess, over, Some(clean), false, "", &global);
        assert!(!panel.rows().contains(&Entry::ResetSite));
        panel.open(Strip::QuickAccess, over, None, false, "", &config);
        assert!(!(0..panel.rows().len()).any(|i| panel.own(i)));
    }

    /// A site leaves every row of the tab's where it was: its reader view takes
    /// the tab's place, launch and reset join the settings before the way out.
    #[test]
    fn a_site_keeps_the_tabs_rows_in_place() {
        let config = AppConfig::default();
        for (mode, launched) in [(TabMode::Page, false), (TabMode::Game, true)] {
            let over = Over::Tab(mode);
            let mut panel = QuickAccess::new();
            panel.open(
                Strip::QuickAccess,
                over,
                None,
                launched,
                "example.com",
                &config,
            );
            let plain = panel.rows().to_vec();
            let breaks = panel.breaks().to_vec();
            assert_eq!(panel.title(), "example.com");
            assert_eq!(panel.bookmark_id(), None);
            panel.open(
                Strip::QuickAccess,
                over,
                Some(itch()),
                launched,
                "example.com",
                &config,
            );
            let out = way_out(Strip::QuickAccess, over, launched);
            let mut want: Vec<Entry> = plain
                .iter()
                .filter(|row| Some(**row) != out)
                .map(|row| match row {
                    Entry::Run(Action::Reader) => Entry::Options(OptionsRow::Reader),
                    row => *row,
                })
                .collect();
            want.extend([Entry::Options(OptionsRow::LaunchAs), Entry::ResetSite]);
            want.extend(out);
            assert_eq!(panel.rows(), want);
            assert_eq!(panel.breaks(), breaks);
            assert_eq!(panel.title(), "itch.io");
            assert_eq!(panel.bookmark_id(), Some("itch"));
        }
    }

    /// A game's site steps how it launches, with no reader view.
    #[test]
    fn a_games_site_launches_without_reader_view() {
        let mut panel = QuickAccess::new();
        let over = Over::Tab(TabMode::Game);
        panel.open(
            Strip::QuickAccess,
            over,
            Some(itch()),
            true,
            "",
            &AppConfig::default(),
        );
        assert_eq!(panel.row(), Entry::Reload);
        assert!(panel.rows().contains(&Entry::Options(OptionsRow::LaunchAs)));
        assert!(!panel.rows().contains(&Entry::Options(OptionsRow::Reader)));
        panel.open(
            Strip::QuickMenu,
            over,
            Some(itch()),
            true,
            "",
            &AppConfig::default(),
        );
        assert_eq!(panel.title(), Strip::QuickMenu.name());
        assert!(!panel.rows().iter().any(|r| matches!(r, Entry::Options(_))));
        assert_eq!(panel.bookmark_id(), None);
    }

    /// Every quick row over a tab writes only site settings, which a site's
    /// bookmark takes.
    #[test]
    fn site_rows_set_only_site_settings() {
        let base = AppConfig::default();
        for field in Settings::fields() {
            let over_page = field
                .quick
                .is_some_and(|m| m.includes(Over::Tab(TabMode::Page)));
            let over_game = field
                .quick
                .is_some_and(|m| m.includes(Over::Tab(TabMode::Game)));
            if !(over_page || over_game) {
                continue;
            }
            for dx in [-1, 1] {
                let mut config = base.clone();
                settings::step(&field.kind, &mut config, dx);
                let changed = crate::config::overlay::changes(&config, &base);
                let site =
                    crate::config::overlay::pick(&changed, crate::config::overlay::SITE_KEYS);
                assert_eq!(changed, site, "{}", field.label);
            }
        }
    }

    /// The way out names the bookmark it closes.
    #[test]
    fn the_way_out_names_the_launched_bookmark() {
        let mut panel = QuickAccess::new();
        let over = Over::Tab(TabMode::Game);
        let site = SiteHead {
            id: "hexgl",
            name: "HexGL",
            ..itch()
        };
        panel.open(
            Strip::QuickAccess,
            over,
            Some(site),
            true,
            "",
            &AppConfig::default(),
        );
        let last = panel.rows().len() - 1;
        assert_eq!(panel.label(last), "Close HexGL");
        let site = SiteHead { name: "", ..site };
        panel.open(
            Strip::QuickAccess,
            over,
            Some(site),
            true,
            "",
            &AppConfig::default(),
        );
        assert_eq!(panel.label(last), "Close");
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
        panel.open_home(Some(tile(true, false)), None, &AppConfig::default());
        let options = options_rows(true, false);
        assert_eq!(&panel.rows()[..options.len()], &options[..]);
        assert!(panel.rows()[options.len()..]
            .iter()
            .all(|row| matches!(row, Entry::Quick(_))));
        assert_eq!(
            panel.breaks(),
            [(options.len(), Break::Head(HOME_HEAD.to_string()))]
        );
        assert_eq!(panel.title(), "A");
        assert_eq!(panel.value(0), "a.example");
        panel.set_title("B".into());
        assert_eq!(panel.title(), "B");
        panel.open_home(None, None, &AppConfig::default());
        assert!(panel.breaks().is_empty());
        assert_eq!(panel.title(), HOME_HEAD);
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
        panel.open_home(Some(tile(true, false)), None, &AppConfig::default());
        assert!(moves(&panel) && panel.bookmark_id() == Some("a"));
        panel.open_home(Some(tile(false, true)), None, &AppConfig::default());
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
            None,
            false,
            "",
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
        // A row that stays up once confirmed drops its hint.
        assert_eq!(panel.value(panel.selected()), "");
        assert!(!panel.confirm(Entry::Reload));
    }

    /// Regression: removing the page's bookmark closed the panel before its
    /// second press.
    #[test]
    fn the_bookmark_row_arms_its_removal() {
        let mut panel = open(TabMode::Page);
        let row = Entry::Run(Action::Bookmark);
        panel.select_entry(row);
        assert!(!panel.confirm(row));
        assert_eq!(panel.value(panel.selected()), "press again to remove");
        assert!(panel.confirm(row));
    }

    /// [`crate::event::bindings::REQUIRED`] counts on Quick Menu to reach the
    /// settings wherever it opens.
    #[test]
    fn quick_menu_reaches_the_settings_everywhere() {
        let overs = [
            Over::Tab(TabMode::Page),
            Over::Tab(TabMode::Reader),
            Over::Tab(TabMode::Game),
            Over::Home,
        ];
        for over in overs {
            for launched in [false, true] {
                let menu = entries(Strip::QuickMenu, over, launched);
                assert!(menu.contains(&Entry::Settings), "{over:?}");
            }
        }
    }

    /// A rule parts what acts on a tab from what sets it; Quick Menu is one run.
    #[test]
    fn a_rule_parts_actions_from_settings() {
        for mode in [TabMode::Page, TabMode::Game] {
            let panel = open(mode);
            let settings = panel
                .rows()
                .iter()
                .position(|row| matches!(row, Entry::Quick(_)));
            let rule = settings.expect("every tab strip sets something");
            assert_eq!(panel.breaks(), [(rule, Break::Rule)], "{mode:?}");
        }
        let mut panel = QuickAccess::new();
        let over = Over::Tab(TabMode::Page);
        panel.open(
            Strip::QuickMenu,
            over,
            None,
            false,
            "",
            &AppConfig::default(),
        );
        assert!(panel.breaks().is_empty());
    }

    /// The header names the page's site, else the strip; Quick Menu always
    /// names itself.
    #[test]
    fn the_header_names_what_the_rows_act_on() {
        let mut panel = QuickAccess::new();
        let over = Over::Tab(TabMode::Page);
        let config = AppConfig::default();
        panel.open(
            Strip::QuickAccess,
            over,
            None,
            false,
            "example.com",
            &config,
        );
        assert_eq!(panel.title(), "example.com");
        panel.open(Strip::QuickAccess, over, None, false, "", &config);
        assert_eq!(panel.title(), Strip::QuickAccess.name());
        let removed = SiteHead {
            id: "a",
            name: "",
            ..itch()
        };
        panel.open(
            Strip::QuickAccess,
            over,
            Some(removed),
            false,
            "example.com",
            &config,
        );
        assert_eq!(panel.title(), "example.com");
        panel.open(Strip::QuickMenu, over, None, false, "example.com", &config);
        assert_eq!(panel.title(), Strip::QuickMenu.name());
    }

    /// What the footer says A does follows the focused row and its state.
    #[test]
    fn the_press_follows_the_focused_row() {
        let press_on = |panel: &mut QuickAccess, label: &str| {
            let at = (0..panel.rows().len()).find(|&i| panel.label(i) == label);
            panel.select(at.expect("a tab strip row"));
            panel.press()
        };
        let mut panel = open(TabMode::Page);
        assert_eq!(press_on(&mut panel, "Reload"), Press::Select);
        assert_eq!(press_on(&mut panel, "Ad blocker"), Press::Switch);
        assert_eq!(press_on(&mut panel, "Reader view"), Press::Switch);
        assert_eq!(press_on(&mut panel, "User agent"), Press::Next);
        assert_eq!(press_on(&mut panel, "JS engine"), Press::Next);
        let config = AppConfig::default();
        // The page opened under another preset than the one in force.
        let opened = format!("not-{}", config.performance.js_preset);
        panel.set_page_js(Some(opened));
        panel.refresh(&config);
        assert_eq!(press_on(&mut panel, "JS engine"), Press::Reopen);
        let mut panel = open(TabMode::Game);
        assert_eq!(press_on(&mut panel, "Input map"), Press::Open);
        assert_eq!(press_on(&mut panel, "Reload"), Press::Select);
        panel.confirm(Entry::Reload);
        assert_eq!(panel.press(), Press::Confirm);
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
