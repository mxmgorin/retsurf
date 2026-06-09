//! The full-screen menu opened with Select (or the ☰ toolbar button): a tabbed
//! overlay over the page with **Tabs · Bookmarks · History** sections. It owns the
//! overlay state (whether it's shown, which section is active) and the Bookmarks
//! and History stores. The central router ([`crate::app`]) maps gamepad / keyboard
//! / mouse input to section switches, selection moves, open, delete, and clear;
//! [`crate::ui`] renders it. Tabs is a placeholder until multi-tab support lands.

use crate::bookmarks::Bookmarks;
use crate::config::HistoryConfig;
use crate::history::History;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Tabs,
    Bookmarks,
    History,
}

impl Section {
    /// Left-to-right order of the section bar.
    pub const ALL: [Section; 3] = [Section::Tabs, Section::Bookmarks, Section::History];

    pub fn label(self) -> &'static str {
        match self {
            Section::Tabs => "Tabs",
            Section::Bookmarks => "Bookmarks",
            Section::History => "History",
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
    history: History,
}

impl Menu {
    pub fn new(history_cfg: &HistoryConfig) -> Self {
        Self {
            visible: false,
            // Open on Bookmarks: it's the most useful section and Tabs is still a
            // placeholder.
            section: Section::Bookmarks,
            bookmarks: Bookmarks::load(),
            history: History::load(history_cfg),
        }
    }

    /// Show the menu, resetting both lists' highlights to the top.
    pub fn open(&mut self) {
        self.visible = true;
        self.bookmarks.reset();
        self.history.reset();
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

    /// Move the active section's selection by `dy` rows (Tabs has none yet).
    pub fn move_sel(&mut self, dy: i32) {
        match self.section {
            Section::Bookmarks => self.bookmarks.move_sel(dy),
            Section::History => self.history.move_sel(dy),
            Section::Tabs => {}
        }
    }

    /// URL of the highlighted entry in the active section, if any (Tabs: none yet).
    pub fn selected_url(&self) -> Option<String> {
        match self.section {
            Section::Bookmarks => self.bookmarks.selected_url(),
            Section::History => self.history.selected_url(),
            Section::Tabs => None,
        }
    }

    /// Remove the highlighted entry in the active section.
    pub fn remove_selected(&mut self) {
        match self.section {
            Section::Bookmarks => self.bookmarks.remove_selected(),
            Section::History => self.history.remove_selected(),
            Section::Tabs => {}
        }
    }

    /// Remove the entry at `index` in the active section (clicking its ✖).
    pub fn remove_at(&mut self, index: usize) {
        match self.section {
            Section::Bookmarks => self.bookmarks.remove(index),
            Section::History => self.history.remove(index),
            Section::Tabs => {}
        }
    }

    /// Clear all entries in the active section (only History offers this today).
    pub fn clear(&mut self) {
        if self.section == Section::History {
            self.history.clear();
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
