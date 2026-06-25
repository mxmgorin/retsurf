//! Saved bookmarks: a flat list of URLs persisted to `bookmarks.toml` in the
//! user data dir, plus the highlighted row in the menu's Bookmarks section. The
//! menu (see [`crate::overlay::menu`]) owns whether the overlay is shown; this just owns
//! the list and selection. Rendered by [`crate::ui`], driven by the central router.

use serde::{Deserialize, Serialize};

/// On-disk shape (a TOML table can't be a bare array, so wrap the list).
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    urls: Vec<String>,
}

pub struct Bookmarks {
    urls: Vec<String>,
    /// Highlighted row in the menu's Bookmarks section.
    cursor: super::ListCursor,
}

impl Bookmarks {
    /// Load the saved list (missing/invalid file → empty).
    pub fn load() -> Self {
        let urls = super::load_toml::<Store>("bookmarks.toml").urls;
        Self {
            urls,
            cursor: super::ListCursor::new(0),
        }
    }

    /// Best-effort persist; failures are logged, not fatal.
    fn save(&self) {
        let store = Store {
            urls: self.urls.clone(),
        };
        super::save_toml("bookmarks.toml", &store, "bookmarks");
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    pub fn selected(&self) -> usize {
        self.cursor.selected()
    }

    pub fn contains(&self, url: &str) -> bool {
        self.urls.iter().any(|u| u == url)
    }

    /// Add `url` if absent, otherwise remove it; persists either way.
    pub fn toggle(&mut self, url: &str) {
        if let Some(i) = self.urls.iter().position(|u| u == url) {
            self.urls.remove(i);
            self.clamp_selected();
        } else {
            self.urls.push(url.to_string());
        }
        self.save();
    }

    /// Reset the highlight to the top (called when the menu opens).
    pub fn reset(&mut self) {
        self.cursor.reset(self.urls.len());
    }

    /// Move the highlight by `dy` rows, clamped to the list.
    pub fn move_sel(&mut self, dy: i32) {
        self.cursor.move_sel(dy, self.urls.len());
    }

    pub fn selected_url(&self) -> Option<String> {
        self.cursor
            .entry_index()
            .and_then(|i| self.urls.get(i))
            .cloned()
    }

    /// Remove the highlighted entry; persists.
    pub fn remove_selected(&mut self) {
        if let Some(i) = self.cursor.entry_index() {
            self.remove(i);
        }
    }

    /// Remove the entry at `index` (if in range); persists.
    pub fn remove(&mut self, index: usize) {
        if index < self.urls.len() {
            self.urls.remove(index);
            self.clamp_selected();
            self.save();
        }
    }

    fn clamp_selected(&mut self) {
        self.cursor.clamp(self.urls.len());
    }
}
