//! The full-screen menu opened with Select (or the ☰ toolbar button): a tabbed
//! overlay over the page with **Tabs · Bookmarks · History · Downloads** sections.
//! It owns the overlay state (whether it's shown, which section is active) and the
//! Bookmarks, History, and Downloads stores. The central router ([`crate::app`])
//! maps gamepad / keyboard / mouse input to section switches, selection moves,
//! open, delete, and clear; [`crate::ui`] renders it.

use crate::config::{DownloadsConfig, HistoryConfig};
use crate::data::bookmarks::Bookmarks;
use crate::data::dial::Dial;
use crate::data::downloads::Downloads;
use crate::data::history::History;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Tabs,
    Bookmarks,
    History,
    Downloads,
}

impl Section {
    /// Left-to-right order of the section bar.
    pub const ALL: [Section; 4] = [
        Section::Tabs,
        Section::Bookmarks,
        Section::History,
        Section::Downloads,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Tabs => "Tabs",
            Section::Bookmarks => "Bookmarks",
            Section::History => "History",
            Section::Downloads => "Downloads",
        }
    }

    fn index(self) -> usize {
        Section::ALL.iter().position(|s| *s == self).unwrap()
    }
}

pub struct Menu {
    pub visible: bool,
    section: Section,
    bookmarks: Bookmarks,
    /// The start page's pinned speed-dial (separate from bookmarks; pinned from
    /// the Bookmarks / History sections with Y). Lives here so both the menu's
    /// pin action and the start-page renderer ([`crate::ui::home`]) share it.
    pub dial: Dial,
    history: History,
    pub downloads: Downloads,
    /// Highlighted row in the Tabs section. The tab list lives in the browser, so
    /// this index is clamped against `tab_count`, refreshed each frame the menu is
    /// shown. The row at index `tab_count` is the "+ New tab" entry.
    tab_selected: usize,
    tab_count: usize,
}

impl Menu {
    pub fn new(history_cfg: &HistoryConfig, downloads_cfg: &DownloadsConfig) -> Self {
        Self {
            visible: false,
            section: Section::Tabs,
            bookmarks: Bookmarks::load(),
            dial: Dial::load(),
            history: History::load(history_cfg),
            downloads: Downloads::load(downloads_cfg),
            tab_selected: 0,
            tab_count: 0,
        }
    }

    /// Show the menu, resetting every section's highlight to the top. In Tabs,
    /// index 0 is the "+ New tab" button, so the cursor starts on the first tab
    /// (index 1) — A then switches tabs rather than spawning a new one.
    pub fn open(&mut self) {
        self.visible = true;
        self.bookmarks.reset();
        self.history.reset();
        self.downloads.reset();
        self.tab_selected = 1;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn bookmarks(&self) -> &Bookmarks {
        &self.bookmarks
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Switch the active section by `delta` (clamped to the ends, no wrap).
    pub fn switch_section(&mut self, delta: i32) {
        let last = Section::ALL.len() as i32 - 1;
        let i = (self.section.index() as i32 + delta).clamp(0, last) as usize;
        self.section = Section::ALL[i];
    }

    /// Jump straight to a section (clicking its tab).
    pub fn set_section(&mut self, section: Section) {
        self.section = section;
    }

    /// Move the active section's selection by `dy` rows.
    pub fn move_sel(&mut self, dy: i32) {
        match self.section {
            Section::Bookmarks => self.bookmarks.move_sel(dy),
            Section::History => self.history.move_sel(dy),
            Section::Downloads => self.downloads.move_sel(dy),
            // Index 0 is the "+ New tab" button; the tabs follow at `1..=tab_count`.
            Section::Tabs => {
                let last = self.tab_count as i32;
                self.tab_selected = (self.tab_selected as i32 + dy).clamp(0, last) as usize;
            }
        }
    }

    /// Highlighted row in the Tabs section (0 == the "+ New tab" button, then
    /// the tabs at `1..=tab_count`).
    pub fn tab_selected(&self) -> usize {
        self.tab_selected
    }

    /// Whether the History section's "Clear all" top row is highlighted (A then
    /// clears, mirroring how A on the Tabs "+ New tab" row opens a tab).
    pub fn history_clear_selected(&self) -> bool {
        self.section == Section::History && self.history.clear_selected()
    }

    /// Refresh the known tab count (the tab list lives in the browser), keeping the
    /// Tabs selection in range. Called each frame the menu is shown.
    pub fn set_tab_count(&mut self, count: usize) {
        self.tab_count = count;
        if self.tab_selected > count {
            self.tab_selected = count;
        }
    }

    /// URL of the highlighted entry in the active section, if any (Tabs: none;
    /// Downloads: the `file://` URL of a successfully finished entry).
    pub fn selected_url(&self) -> Option<String> {
        match self.section {
            Section::Bookmarks => self.bookmarks.selected_url(),
            Section::History => self.history.selected_url(),
            Section::Downloads => self.downloads.selected_open_url(),
            Section::Tabs => None,
        }
    }

    /// Remove the highlighted entry in the active section (Downloads: cancels the
    /// entry instead while it's still in flight).
    pub fn remove_selected(&mut self) {
        match self.section {
            Section::Bookmarks => self.bookmarks.remove_selected(),
            Section::History => self.history.remove_selected(),
            Section::Downloads => self.downloads.remove_selected(),
            Section::Tabs => {}
        }
    }

    /// Remove the entry at `index` in the active section (clicking its ✖).
    pub fn remove_at(&mut self, index: usize) {
        match self.section {
            Section::Bookmarks => self.bookmarks.remove(index),
            Section::History => self.history.remove(index),
            Section::Downloads => self.downloads.remove(index),
            Section::Tabs => {}
        }
    }

    /// Clear the active section's list: all history entries, or all finished
    /// downloads (active ones stay).
    pub fn clear(&mut self) {
        match self.section {
            Section::History => self.history.clear(),
            Section::Downloads => self.downloads.clear_finished(),
            Section::Tabs | Section::Bookmarks => {}
        }
    }

    pub fn record_history(&mut self, url: &str) {
        self.history.record(url);
    }

    /// Whether `url` is a saved bookmark (drives the ★/☆ toolbar icon).
    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks.contains(url)
    }

    /// Add or remove `url` from saved bookmarks (the ★ button / Start).
    pub fn toggle_bookmark(&mut self, url: &str) {
        self.bookmarks.toggle(url);
    }
}
