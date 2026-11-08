//! The search-engine list behind the Browser tab's door row, over the draft
//! config: a row that adds an engine, then the engines. One engine's own
//! screen uses, renames, re-points or deletes it, a deletion only after a
//! confirmation. Names and URLs are typed on the on-screen keyboard, and only
//! Enter commits one ([`Settings::engine_text`]).

use super::{Draft, Settings};
use crate::config::BrowserConfig;

/// What one engine's screen offers, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineAction {
    Use,
    Rename,
    Url,
    Delete,
}

impl EngineAction {
    pub fn label(self) -> &'static str {
        match self {
            EngineAction::Use => "Use this engine",
            EngineAction::Rename => "Rename",
            EngineAction::Url => "Edit URL",
            EngineAction::Delete => "Delete",
        }
    }
}

/// A row of whichever level is up.
#[derive(Clone, PartialEq, Debug)]
pub enum EngineRow {
    /// The list's row that adds an engine, with the name whose URL is being
    /// asked for, if any, and what is typed.
    New {
        url_for: Option<String>,
        typed: String,
    },
    Engine {
        name: String,
        in_use: bool,
    },
    /// An open engine's row, with the value it shows.
    Take(EngineAction, String),
    /// The confirmation's answers: `true` deletes.
    Answer(bool),
}

/// The level that is up, and the engine it is about.
#[derive(Clone, PartialEq, Debug)]
pub enum EngineTitle {
    List,
    Engine(String),
    /// The deletion of this engine is being asked about.
    Confirm(String),
}

/// What the keyboard is typing.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Typing {
    /// A new engine's name; its URL is asked next.
    NewName,
    /// The URL of the new engine of this name.
    NewUrl(String),
    /// The open engine's name.
    Rename,
    /// The open engine's URL.
    Url,
}

/// What a new engine's URL starts from on the keyboard.
const URL_START: &str = "https://";

/// The confirmation's rows: Delete, then Cancel, where the highlight starts.
const CONFIRM_ROWS: usize = 2;
const CANCEL_ROW: usize = 1;

/// The list shows the New row first, so the engines start one row below
/// their index in the config.
const ENGINES_START: usize = 1;

/// Where the screen stands. Lives while the screen is up.
#[derive(Default)]
pub(super) struct Engines {
    /// The highlight on the list, the New row included.
    selected: usize,
    /// The engine whose own screen is up.
    open: Option<usize>,
    /// The highlight on that screen.
    at: usize,
    /// The highlight on the deletion's confirmation, while it is up.
    confirm: Option<usize>,
    /// What the keyboard types, and the text so far.
    typing: Option<(Typing, String)>,
}

/// What engine `i`'s screen offers. `Use` is absent on the engine in use and
/// `Delete` on the last one: a row that would do nothing is not there.
fn actions(browser: &BrowserConfig, i: usize) -> Vec<EngineAction> {
    let mut actions = Vec::new();
    if browser.search_index() != Some(i) {
        actions.push(EngineAction::Use);
    }
    actions.extend([EngineAction::Rename, EngineAction::Url]);
    if browser.search_engines.len() > 1 {
        actions.push(EngineAction::Delete);
    }
    actions
}

impl Engines {
    /// The highlight on whichever level is up.
    pub(super) fn cursor(&self) -> usize {
        match (self.confirm, self.open) {
            (Some(at), _) => at,
            (None, Some(_)) => self.at,
            (None, None) => self.selected,
        }
    }

    fn set_cursor(&mut self, to: usize) {
        match (&mut self.confirm, self.open) {
            (Some(at), _) => *at = to,
            (None, Some(_)) => self.at = to,
            (None, None) => self.selected = to,
        }
    }

    /// How many rows the level that is up has.
    fn len(&self, browser: &BrowserConfig) -> usize {
        match (self.confirm, self.open) {
            (Some(_), _) => CONFIRM_ROWS,
            (None, Some(i)) => actions(browser, i).len(),
            (None, None) => ENGINES_START + browser.search_engines.len(),
        }
    }

    /// Move the highlight by `dy`, clamped to the level's ends.
    pub(super) fn move_sel(&mut self, dy: i32, browser: &BrowserConfig) {
        let to = crate::list::step(self.cursor(), dy, self.len(browser));
        self.set_cursor(to);
    }

    /// Highlight row `i` of the level that is up, if it has one.
    pub(super) fn select(&mut self, i: usize, browser: &BrowserConfig) {
        if i < self.len(browser) {
            self.set_cursor(i);
        }
    }

    /// Show engine `i`'s screen, highlighting it on the list behind.
    fn open(&mut self, i: usize) {
        self.selected = i + ENGINES_START;
        self.open = Some(i);
        self.at = 0;
    }

    /// Back out one level: the typed text, the confirmation, then the engine.
    /// `false` once only the list is left.
    pub(super) fn back(&mut self) -> bool {
        if self.typing.take().is_some() || self.confirm.take().is_some() {
            return true;
        }
        self.open.take().is_some()
    }

    /// Keep every highlight on a row of the draft as it now stands.
    fn clamp(&mut self, browser: &BrowserConfig) {
        self.selected = self.selected.min(browser.search_engines.len());
        if let Some(i) = self.open {
            self.at = self.at.min(actions(browser, i).len().saturating_sub(1));
        }
    }

    /// Act on the highlighted row; returns whether the keyboard is to come up.
    fn press(&mut self, browser: &mut BrowserConfig) -> bool {
        match (self.confirm, self.open) {
            (Some(at), Some(i)) => {
                self.confirm = None;
                if at != CANCEL_ROW && browser.remove_search_engine(i) {
                    self.open = None;
                }
            }
            (None, Some(i)) => match actions(browser, i).get(self.at) {
                Some(EngineAction::Use) => browser.use_search_engine(i),
                Some(EngineAction::Rename) => {
                    let name = browser.search_engines[i].name.clone();
                    self.typing = Some((Typing::Rename, name));
                }
                Some(EngineAction::Url) => {
                    let url = browser.search_engines[i].url.clone();
                    self.typing = Some((Typing::Url, url));
                }
                Some(EngineAction::Delete) => self.confirm = Some(CANCEL_ROW),
                None => {}
            },
            (None, None) => match self.selected.checked_sub(ENGINES_START) {
                None => self.typing = Some((Typing::NewName, String::new())),
                Some(i) if i < browser.search_engines.len() => self.open(i),
                Some(_) => {}
            },
            (Some(_), None) => self.confirm = None,
        }
        self.clamp(browser);
        self.typing.is_some()
    }

    /// Commit what the keyboard submitted; returns whether it is to come up
    /// again, as it does for a new engine's URL after its name.
    fn commit(&mut self, text: &str, browser: &mut BrowserConfig) -> bool {
        let Some((typing, _)) = self.typing.take() else {
            return false;
        };
        match (typing, self.open) {
            (Typing::NewName, _) if !text.is_empty() => {
                let typing = Typing::NewUrl(text.to_string());
                self.typing = Some((typing, URL_START.to_string()));
            }
            (Typing::NewUrl(name), _) => {
                if let Some(i) = browser.add_search_engine(&name, text) {
                    self.open(i);
                }
            }
            (Typing::Rename, Some(i)) => browser.rename_search_engine(i, text),
            (Typing::Url, Some(i)) => browser.set_search_url(i, text),
            _ => {}
        }
        self.typing.is_some()
    }

    /// The level that is up and its rows.
    fn screen(&self, browser: &BrowserConfig) -> Option<(EngineTitle, Vec<EngineRow>)> {
        let typed = |what: &Typing| match &self.typing {
            Some((typing, text)) if typing == what => Some(text.clone()),
            _ => None,
        };
        let Some(i) = self.open else {
            let (url_for, typed) = match &self.typing {
                Some((Typing::NewUrl(name), text)) => (Some(name.clone()), text.clone()),
                Some((Typing::NewName, text)) => (None, text.clone()),
                _ => (None, String::new()),
            };
            let in_use = browser.search_index();
            let engines = browser.search_engines.iter().enumerate().map(|(at, e)| {
                let in_use = in_use == Some(at);
                EngineRow::Engine {
                    name: e.name.clone(),
                    in_use,
                }
            });
            let rows = std::iter::once(EngineRow::New { url_for, typed })
                .chain(engines)
                .collect();
            return Some((EngineTitle::List, rows));
        };
        let engine = browser.search_engines.get(i)?;
        if self.confirm.is_some() {
            let rows = vec![EngineRow::Answer(true), EngineRow::Answer(false)];
            return Some((EngineTitle::Confirm(engine.name.clone()), rows));
        }
        let rows = actions(browser, i)
            .into_iter()
            .map(|action| {
                let value = match action {
                    EngineAction::Rename => typed(&Typing::Rename).unwrap_or_default(),
                    EngineAction::Url => typed(&Typing::Url).unwrap_or(engine.url.clone()),
                    EngineAction::Use | EngineAction::Delete => String::new(),
                };
                EngineRow::Take(action, value)
            })
            .collect();
        Some((EngineTitle::Engine(engine.name.clone()), rows))
    }
}

impl Settings {
    /// Whether the search-engine list is up in place of the Browser tab's fields.
    pub fn engines_open(&self) -> bool {
        self.draft().is_some_and(|draft| draft.engines.is_some())
    }

    /// The level that is up and its rows; `None` while the list is not up.
    pub fn engine_screen(&self) -> Option<(EngineTitle, Vec<EngineRow>)> {
        let draft = self.draft()?;
        draft.engines.as_ref()?.screen(&draft.config.browser)
    }

    /// Act on the highlighted row: open an engine, take one of its rows, or
    /// answer the confirmation. Returns whether the keyboard is to come up.
    pub fn engines_activate(&mut self) -> bool {
        let Some(Draft {
            engines: Some(engines),
            config,
            ..
        }) = self.draft_mut()
        else {
            return false;
        };
        engines.press(&mut config.browser)
    }

    /// Commit typed `text`; returns whether the keyboard is to come up again.
    pub fn engine_text(&mut self, text: &str) -> bool {
        let Some(Draft {
            engines: Some(engines),
            config,
            ..
        }) = self.draft_mut()
        else {
            return false;
        };
        engines.commit(text, &mut config.browser)
    }

    /// Whether the keyboard types for the list.
    pub fn engine_typing(&self) -> bool {
        self.draft()
            .and_then(|draft| draft.engines.as_ref())
            .is_some_and(|engines| engines.typing.is_some())
    }

    /// The buffer the keyboard types into.
    pub fn engine_text_mut(&mut self) -> Option<&mut String> {
        let engines = self.draft_mut()?.engines.as_mut()?;
        engines.typing.as_mut().map(|(_, text)| text)
    }

    /// Drop what the keyboard typed without committing it.
    pub fn drop_engine_typing(&mut self) {
        if let Some(engines) = self.draft_mut().and_then(|draft| draft.engines.as_mut()) {
            engines.typing = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    /// Settings with the engine list up.
    fn opened() -> Settings {
        let mut settings = Settings::new();
        settings.open(&AppConfig::default());
        let door = super::super::fields::FIELDS
            .iter()
            .position(|f| {
                matches!(
                    f.kind,
                    super::super::Kind::Door {
                        door: super::super::Door::SearchEngines
                    }
                )
            })
            .expect("FIELDS lists the engine list's door");
        settings.set_selected(door);
        settings.open_door();
        assert!(settings.engines_open());
        settings
    }

    fn screen(settings: &Settings) -> (EngineTitle, Vec<EngineRow>) {
        settings.engine_screen().expect("the list is up")
    }

    fn browser(settings: &Settings) -> &BrowserConfig {
        &settings.draft().expect("open").config.browser
    }

    #[test]
    fn the_list_leads_with_the_row_that_adds_and_marks_the_engine_in_use() {
        let settings = opened();
        let (title, rows) = screen(&settings);
        assert_eq!(title, EngineTitle::List);
        assert!(matches!(rows[0], EngineRow::New { url_for: None, .. }));
        assert!(matches!(&rows[1], EngineRow::Engine { in_use: true, .. }));
        assert!(matches!(&rows[2], EngineRow::Engine { in_use: false, .. }));
        assert_eq!(rows.len(), 1 + browser(&settings).search_engines.len());
    }

    #[test]
    fn a_new_engine_takes_a_name_then_a_url_and_opens() {
        let mut settings = opened();
        assert!(settings.engines_activate(), "the New row asks for a name");
        assert!(settings.engine_typing());
        assert!(settings.engine_text("Kagi"), "and then for the URL");
        assert_eq!(
            settings.engine_text_mut().map(|t| t.clone()),
            Some(URL_START.to_string())
        );
        assert!(!settings.engine_text("https://kagi.com/search?q="));

        let (title, rows) = screen(&settings);
        assert_eq!(title, EngineTitle::Engine("Kagi".to_string()));
        assert_eq!(rows[0], EngineRow::Take(EngineAction::Use, String::new()));
        let kagi = browser(&settings).search_engines.last().expect("added");
        assert_eq!(kagi.url, "https://kagi.com/search?q=%s");
    }

    #[test]
    fn a_blank_name_adds_nothing() {
        let mut settings = opened();
        let count = browser(&settings).search_engines.len();
        settings.engines_activate();
        assert!(!settings.engine_text(""));
        assert!(!settings.engine_typing());
        assert_eq!(browser(&settings).search_engines.len(), count);
    }

    #[test]
    fn using_an_engine_takes_its_use_row_away() {
        let mut settings = opened();
        settings.move_sel(2, 0);
        settings.engines_activate();
        assert_eq!(
            screen(&settings).0,
            EngineTitle::Engine("DuckDuckGo".to_string())
        );
        settings.engines_activate();
        assert_eq!(browser(&settings).search_engine, "DuckDuckGo");
        let (_, rows) = screen(&settings);
        assert_eq!(
            rows[0],
            EngineRow::Take(EngineAction::Rename, String::new())
        );
    }

    #[test]
    fn a_deletion_is_asked_about_from_cancel() {
        let mut settings = opened();
        settings.move_sel(2, 0);
        settings.engines_activate();
        settings.move_sel(3, 0);
        settings.engines_activate();
        assert_eq!(
            screen(&settings).0,
            EngineTitle::Confirm("DuckDuckGo".to_string())
        );
        assert_eq!(settings.selected(), CANCEL_ROW);
        settings.engines_activate();
        assert_eq!(
            screen(&settings).0,
            EngineTitle::Engine("DuckDuckGo".to_string())
        );

        settings.engines_activate();
        settings.move_sel(-1, 0);
        settings.engines_activate();
        assert_eq!(screen(&settings).0, EngineTitle::List);
        assert!(browser(&settings)
            .search_engines
            .iter()
            .all(|e| e.name != "DuckDuckGo"));
    }

    #[test]
    fn the_last_engine_cannot_be_deleted() {
        let mut settings = opened();
        let draft = settings.draft_mut().expect("open");
        draft.config.browser.search_engines.truncate(1);
        settings.move_sel(1, 0);
        settings.engines_activate();
        let (_, rows) = screen(&settings);
        assert!(rows
            .iter()
            .all(|row| !matches!(row, EngineRow::Take(EngineAction::Delete, _))));
    }

    #[test]
    fn back_pops_one_level_at_a_time() {
        let mut settings = opened();
        settings.move_sel(2, 0);
        settings.engines_activate();
        settings.move_sel(3, 0);
        settings.engines_activate();
        assert!(settings.close_door());
        assert!(matches!(screen(&settings).0, EngineTitle::Engine(_)));
        assert!(settings.close_door());
        assert_eq!(screen(&settings).0, EngineTitle::List);
        assert!(settings.close_door());
        assert!(!settings.engines_open());
    }

    #[test]
    fn a_dismissed_keyboard_commits_nothing() {
        let mut settings = opened();
        settings.move_sel(1, 0);
        settings.engines_activate();
        settings.engines_activate();
        if let Some(text) = settings.engine_text_mut() {
            text.push_str(" typed");
        }
        settings.drop_engine_typing();
        assert_eq!(browser(&settings).search_engines[0].name, "DuckDuckGo Lite");
    }
}
