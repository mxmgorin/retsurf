//! The tab session: which URLs were open and which was shown, persisted to
//! `session.toml` in the user data dir for `[browser] restore_tabs`.
//! Snapshotted on the main loop's flush throttle and at exit, not per
//! navigation.

use crate::browser::TabInfo;
use serde::{Deserialize, Serialize};

const FILE: &str = "session.toml";

/// On-disk shape (a TOML table can't be a bare array, so wrap the list).
#[derive(Default, PartialEq, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    urls: Vec<String>,
    /// Index into `urls` of the tab that was shown; clamped on restore.
    #[serde(default)]
    active: usize,
}

pub struct Session {
    /// What is on disk, so an unchanged tab list writes nothing.
    saved: Store,
}

impl Session {
    /// Load the stored session (missing/invalid file → no tabs).
    pub fn load() -> Self {
        Self {
            saved: super::load_toml::<Store>(FILE),
        }
    }

    /// The tabs to reopen, in their original order.
    pub fn urls(&self) -> &[String] {
        &self.saved.urls
    }

    /// Which of [`Self::urls`] was shown.
    pub fn active(&self) -> usize {
        self.saved.active
    }

    /// Persist a snapshot of the open tabs; an unchanged list writes nothing,
    /// so the throttle can call this every tick.
    pub fn record(&mut self, tabs: &[TabInfo]) {
        let next = snapshot(tabs);
        if next == self.saved {
            return;
        }
        if super::save_toml(FILE, &next, "session") {
            self.saved = next;
        }
    }

    /// Forget the stored session (`restore_tabs` off), so it can't resurface
    /// when the setting is turned back on.
    pub fn discard(&mut self) {
        if self.saved == Store::default() {
            return;
        }
        self.saved = Store::default();
        super::remove(FILE, "session");
    }
}

/// The session-worthy tabs: those with a loaded URL (a tab still on its first
/// load has none), with the active index remapped past the ones dropped.
fn snapshot(tabs: &[TabInfo]) -> Store {
    let mut urls = Vec::with_capacity(tabs.len());
    let mut active = 0;
    for tab in tabs {
        if tab.url.is_empty() {
            continue;
        }
        if tab.active {
            active = urls.len();
        }
        urls.push(tab.url.clone());
    }
    Store { urls, active }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str, active: bool) -> TabInfo {
        TabInfo {
            title: String::new(),
            url: url.to_string(),
            active,
        }
    }

    /// A tab that never finished a load has no URL to save; the active index
    /// must follow the tabs that survive, not their old positions.
    #[test]
    fn snapshot_drops_unloaded_tabs_and_remaps_active() {
        let tabs = [
            tab("", false),
            tab("https://a", false),
            tab("", false),
            tab("https://b", true),
        ];
        let store = snapshot(&tabs);

        assert_eq!(store.urls, ["https://a", "https://b"]);
        assert_eq!(store.active, 1);
    }

    /// Quitting while the active tab is still blank leaves the highlight on the
    /// first restored tab rather than a dropped index.
    #[test]
    fn snapshot_without_an_active_url_falls_back_to_the_first() {
        let store = snapshot(&[tab("https://a", false), tab("", true)]);

        assert_eq!(store.urls, ["https://a"]);
        assert_eq!(store.active, 0);
    }
}
