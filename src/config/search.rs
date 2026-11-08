//! Search engines (`[browser] search_engines`): where typed text that is no
//! address goes. The list is the user's whole set, seeded from [`DEFAULTS`],
//! and `search_engine` names the one in use. Once sanitized the list is never
//! empty and the edits here keep names unique, so a name always resolves.

use super::BrowserConfig;
use serde::{Deserialize, Serialize};

/// Where the query goes in an engine's URL.
pub const QUERY_MARK: &str = "%s";

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SearchEngine {
    pub name: String,
    /// The results page, with [`QUERY_MARK`] standing for the query.
    pub url: String,
}

/// `(name, url)` per engine a fresh config lists; the first is in use.
const DEFAULTS: &[(&str, &str)] = &[
    ("DuckDuckGo Lite", "https://lite.duckduckgo.com/lite/?q=%s"),
    ("DuckDuckGo", "https://duckduckgo.com/?q=%s"),
    ("Google", "https://www.google.com/search?q=%s"),
    ("Bing", "https://www.bing.com/search?q=%s"),
    ("Brave", "https://search.brave.com/search?q=%s"),
    ("Startpage", "https://www.startpage.com/do/search?q=%s"),
    ("Ecosia", "https://www.ecosia.org/search?q=%s"),
    ("Qwant", "https://lite.qwant.com/?q=%s"),
    ("Mojeek", "https://www.mojeek.com/search?q=%s"),
    (
        "Wikipedia",
        "https://en.m.wikipedia.org/w/index.php?search=%s",
    ),
];

pub(super) fn defaults() -> Vec<SearchEngine> {
    DEFAULTS
        .iter()
        .map(|(name, url)| SearchEngine {
            name: name.to_string(),
            url: url.to_string(),
        })
        .collect()
}

pub(super) fn default_name() -> String {
    DEFAULTS[0].0.to_string()
}

impl BrowserConfig {
    /// The index of the engine in use: the one `search_engine` names, else the
    /// first; `None` only for an empty list.
    pub fn search_index(&self) -> Option<usize> {
        let named = self
            .search_engines
            .iter()
            .position(|e| e.name == self.search_engine);
        named.or((!self.search_engines.is_empty()).then_some(0))
    }

    /// The URL searches go to, [`QUERY_MARK`] standing for the query.
    pub fn search_url(&self) -> &str {
        self.search_index()
            .map_or(DEFAULTS[0].1, |i| &self.search_engines[i].url)
    }

    /// Fill an empty list with the defaults and point `search_engine` at an
    /// engine it lists, logging either repair.
    pub(super) fn sanitize_search(&mut self) {
        if self.search_engines.is_empty() {
            log::warn!("config: browser.search_engines is empty; using the defaults");
            self.search_engines = defaults();
        }
        let name = &self.search_engine;
        if !self.search_engines.iter().any(|e| e.name == *name) {
            let first = self.search_engines[0].name.clone();
            log::warn!("config: browser.search_engine = {name} unknown; using {first}");
            self.search_engine = first;
        }
    }

    /// Append an engine and return its index; `None` when the name or the URL
    /// is blank. A taken name gets a number.
    pub fn add_search_engine(&mut self, name: &str, url: &str) -> Option<usize> {
        let name = self.free_name(name, None)?;
        let url = engine_url(url)?;
        self.search_engines.push(SearchEngine { name, url });
        Some(self.search_engines.len() - 1)
    }

    /// Rename engine `i`, keeping it in use if it was. A blank name changes
    /// nothing; a taken one gets a number.
    pub fn rename_search_engine(&mut self, i: usize, name: &str) {
        let Some(name) = self.free_name(name, Some(i)) else {
            return;
        };
        let Some(engine) = self.search_engines.get_mut(i) else {
            return;
        };
        if engine.name == self.search_engine {
            self.search_engine = name.clone();
        }
        engine.name = name;
    }

    /// Point engine `i` at `url`; a blank one changes nothing.
    pub fn set_search_url(&mut self, i: usize, url: &str) {
        if let (Some(engine), Some(url)) = (self.search_engines.get_mut(i), engine_url(url)) {
            engine.url = url;
        }
    }

    pub fn use_search_engine(&mut self, i: usize) {
        if let Some(engine) = self.search_engines.get(i) {
            self.search_engine = engine.name.clone();
        }
    }

    /// Remove engine `i` unless it is the last one; the first left takes over
    /// when it was in use. Returns whether it went.
    pub fn remove_search_engine(&mut self, i: usize) -> bool {
        if self.search_engines.len() <= 1 || i >= self.search_engines.len() {
            return false;
        }
        let in_use = self.search_index() == Some(i);
        self.search_engines.remove(i);
        if in_use {
            self.search_engine = self.search_engines[0].name.clone();
        }
        true
    }

    /// `wanted`, trimmed, numbered past the names other than engine `except`'s;
    /// `None` when blank.
    fn free_name(&self, wanted: &str, except: Option<usize>) -> Option<String> {
        let wanted = wanted.trim();
        if wanted.is_empty() {
            return None;
        }
        let taken = |name: &str| {
            self.search_engines
                .iter()
                .enumerate()
                .any(|(i, e)| Some(i) != except && e.name == name)
        };
        let numbered = (2..).map(|n| format!("{wanted} {n}"));
        std::iter::once(wanted.to_string())
            .chain(numbered)
            .find(|name| !taken(name))
    }
}

/// `url` trimmed, with the query appended where it names no place for it;
/// `None` when blank.
fn engine_url(url: &str) -> Option<String> {
    let url = url.trim();
    match url {
        "" => None,
        _ if url.contains(QUERY_MARK) => Some(url.to_string()),
        _ => Some(format!("{url}{QUERY_MARK}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BrowserConfig {
        BrowserConfig::default()
    }

    #[test]
    fn the_default_is_the_first_engine() {
        let c = config();
        assert_eq!(c.search_index(), Some(0));
        assert_eq!(c.search_url(), DEFAULTS[0].1);
    }

    #[test]
    fn an_unknown_name_falls_back_to_the_first_engine() {
        let mut c = config();
        c.search_engine = "Nope".to_string();
        assert_eq!(c.search_url(), DEFAULTS[0].1);
        c.sanitize_search();
        assert_eq!(c.search_engine, DEFAULTS[0].0);
    }

    #[test]
    fn an_empty_list_is_refilled() {
        let mut c = config();
        c.search_engines.clear();
        assert_eq!(c.search_url(), DEFAULTS[0].1);
        c.sanitize_search();
        assert_eq!(c.search_engines, defaults());
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let mut c = config();
        let i = c
            .add_search_engine(" Google ", "https://g.example/?q=")
            .expect("added");
        assert_eq!(c.search_engines[i].name, "Google 2");
        assert_eq!(c.search_engines[i].url, "https://g.example/?q=%s");
        assert_eq!(c.add_search_engine("  ", "https://x.example/"), None);
        assert_eq!(c.add_search_engine("X", " "), None);
    }

    #[test]
    fn renaming_the_engine_in_use_keeps_it_in_use() {
        let mut c = config();
        c.rename_search_engine(0, "Duck");
        assert_eq!(c.search_engine, "Duck");
        c.rename_search_engine(0, "Duck");
        assert_eq!(c.search_engines[0].name, "Duck", "its own name is no clash");
        c.rename_search_engine(1, "Duck");
        assert_eq!(c.search_engines[1].name, "Duck 2");
        assert_eq!(c.search_engine, "Duck");
    }

    #[test]
    fn removing_the_engine_in_use_hands_over_to_the_first() {
        let mut c = config();
        c.use_search_engine(2);
        assert!(c.remove_search_engine(2));
        assert_eq!(c.search_engine, DEFAULTS[0].0);
        c.search_engines.truncate(1);
        assert!(!c.remove_search_engine(0), "the last engine stays");
    }
}
