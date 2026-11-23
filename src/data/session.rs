//! The tab session: which URLs were open and which was shown, persisted to
//! `session.toml` in the user data dir for `[browser] restore_tabs`.
//! Snapshotted on the main loop's flush throttle and at exit, not per
//! navigation.
//!
//! A restore leaves a mark file on disk until its tabs have run for
//! [`RESTORE_SETTLE`] or the app leaves on purpose. A launch that finds the mark
//! knows the last one died reopening those tabs, and skips them once.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const FILE: &str = "session.toml";

/// The mark of a restore under way.
const RESTORING: &str = "session.restoring";

/// How long restored tabs run under the mark: a death within it is blamed on them.
const RESTORE_SETTLE: Duration = Duration::from_secs(60);

/// A read-only snapshot of a tab: what the session records, and what the Hub's
/// Tabs section shows.
pub struct TabInfo {
    /// Page title, falling back to the URL (then "New tab") when unknown.
    pub title: String,
    /// The tab's current location (the bookmark target for Y in the Hub); may
    /// be empty for a freshly opened tab that hasn't navigated yet.
    pub url: String,
    /// Whether this is the currently shown tab.
    pub active: bool,
    /// Whether it runs a bookmark as an app or a game.
    pub launched: bool,
    pub favicon: Option<crate::browser::Favicon>,
    /// Its page as last painted, kept for the tab grid.
    pub thumbnail: Option<crate::browser::Thumbnail>,
}

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
    /// When the restore under way settles; `None` while no mark is ours.
    settles_at: Option<Instant>,
}

impl Session {
    /// Load the stored session; a missing or invalid file yields no tabs.
    pub fn load() -> Self {
        Self {
            saved: super::load_toml::<Store>(FILE),
            settles_at: None,
        }
    }

    /// Whether the last launch died before its restored tabs settled. Takes the
    /// mark off, so a failed restore skips one launch.
    pub fn take_failed_restore(&mut self) -> bool {
        let failed = std::path::Path::new(&super::data_path(RESTORING)).exists();
        if failed {
            super::remove(RESTORING, "restore mark");
        }
        failed
    }

    /// Leave the mark for a restore about to start.
    pub fn begin_restore(&mut self) {
        match std::fs::write(super::data_path(RESTORING), "") {
            Ok(()) => self.settles_at = Some(Instant::now() + RESTORE_SETTLE),
            Err(e) => log::warn!("could not write restore mark: {e}"),
        }
    }

    /// Take the mark off if the restore has settled by `now`.
    pub fn settle_restore(&mut self, now: Instant) {
        if self.settles_at.is_some_and(|at| at <= now) {
            self.end_restore();
        }
    }

    /// Take the mark off now, settled or not.
    pub fn end_restore(&mut self) {
        if self.settles_at.take().is_some() {
            super::remove(RESTORING, "restore mark");
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
/// load has none), with the active index remapped past the ones dropped. A
/// launched bookmark is recorded as Home, so it cannot start itself.
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
        urls.push(match tab.launched {
            true => crate::browser::HOME_URL.to_string(),
            false => tab.url.clone(),
        });
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
            launched: false,
            favicon: None,
            thumbnail: None,
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

    #[test]
    fn a_launched_tab_comes_back_as_home() {
        let mut game = tab("https://game.example/", true);
        game.launched = true;
        let store = snapshot(&[tab("https://a", false), game]);
        assert_eq!(store.urls, ["https://a", crate::browser::HOME_URL]);
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
