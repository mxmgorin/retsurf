//! The start page's speed-dial: a flat list of pinned URLs persisted to
//! `dial.toml` in the user data dir. Unlike bookmarks (a menu-only list), the
//! dial is what the built-in start page shows as tiles — curated separately so
//! the two don't fight over one list. Entries are pinned *from* the menu's
//! Bookmarks / History sections (Y) and unpinned on the dial itself (X); there
//! is no in-list selection here (the start page owns tile focus, see
//! [`crate::overlay::home`]). A first run with no file ships [`DEFAULTS`].

use crate::config;
use serde::{Deserialize, Serialize};

/// Shipped on first run so the start page isn't empty before anything is pinned.
const DEFAULTS: &[&str] = &[
    "https://duckduckgo.com",
    "https://en.wikipedia.org",
    "https://github.com",
];

/// On-disk shape (a TOML table can't be a bare array, so wrap the list).
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    urls: Vec<String>,
}

pub struct Dial {
    urls: Vec<String>,
}

impl Dial {
    /// Load the saved list. A missing file seeds [`DEFAULTS`] and writes the
    /// template (so there's a file to edit); an invalid one is logged and falls
    /// back to the defaults too.
    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => match toml::from_str::<Store>(&text) {
                Ok(store) => Self { urls: store.urls },
                Err(e) => {
                    log::error!("invalid dial `{}`: {e}; using defaults", Self::path());
                    Self::seeded()
                }
            },
            Err(_) => {
                let dial = Self::seeded();
                dial.save();
                dial
            }
        }
    }

    fn seeded() -> Self {
        Self {
            urls: DEFAULTS.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn path() -> String {
        format!("{}dial.toml", config::data_dir())
    }

    /// Best-effort persist; failures are logged, not fatal.
    fn save(&self) {
        let store = Store {
            urls: self.urls.clone(),
        };
        match toml::to_string_pretty(&store) {
            Ok(text) => {
                if let Err(e) = std::fs::write(Self::path(), text) {
                    log::warn!("could not write dial: {e}");
                }
            }
            Err(e) => log::warn!("could not serialize dial: {e}"),
        }
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    pub fn contains(&self, url: &str) -> bool {
        self.urls.iter().any(|u| u == url)
    }

    /// Pin `url` if absent, otherwise unpin it; persists either way.
    pub fn toggle(&mut self, url: &str) {
        if let Some(i) = self.urls.iter().position(|u| u == url) {
            self.urls.remove(i);
        } else {
            self.urls.push(url.to_string());
        }
        self.save();
    }
}
