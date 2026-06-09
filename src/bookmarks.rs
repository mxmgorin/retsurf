//! Saved bookmarks: a flat list of URLs persisted to `bookmarks.toml` in the
//! user data dir, plus the state of the full-screen bookmarks overlay (which
//! entry is selected, whether it's shown). Rendered by [`crate::ui`] and driven
//! by the central router.

use crate::config;
use serde::{Deserialize, Serialize};

/// On-disk shape (a TOML table can't be a bare array, so wrap the list).
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    urls: Vec<String>,
}

pub struct Bookmarks {
    urls: Vec<String>,
    /// Whether the bookmarks overlay is shown.
    pub visible: bool,
    /// Highlighted row in the overlay.
    selected: usize,
}

impl Bookmarks {
    /// Load the saved list (missing/invalid file → empty).
    pub fn load() -> Self {
        let urls = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| toml::from_str::<Store>(&text).ok())
            .map(|store| store.urls)
            .unwrap_or_default();
        Self {
            urls,
            visible: false,
            selected: 0,
        }
    }

    fn path() -> String {
        format!("{}bookmarks.toml", config::data_dir())
    }

    /// Best-effort persist; failures are logged, not fatal.
    fn save(&self) {
        let store = Store {
            urls: self.urls.clone(),
        };
        match toml::to_string_pretty(&store) {
            Ok(text) => {
                if let Err(e) = std::fs::write(Self::path(), text) {
                    log::warn!("could not write bookmarks: {e}");
                }
            }
            Err(e) => log::warn!("could not serialize bookmarks: {e}"),
        }
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    pub fn selected(&self) -> usize {
        self.selected
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

    pub fn show(&mut self) {
        self.visible = true;
        self.selected = 0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Move the highlight by `dy` rows, clamped to the list.
    pub fn move_sel(&mut self, dy: i32) {
        if self.urls.is_empty() {
            return;
        }
        let last = self.urls.len() as i32 - 1;
        self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
    }

    pub fn selected_url(&self) -> Option<String> {
        self.urls.get(self.selected).cloned()
    }

    /// Remove the highlighted entry; persists.
    pub fn remove_selected(&mut self) {
        self.remove(self.selected);
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
        self.selected = self.selected.min(self.urls.len().saturating_sub(1));
    }
}
