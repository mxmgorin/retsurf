//! User data stores, all shaped alike: an in-memory list with a highlighted row
//! for the menu, persisted as TOML in the user data dir (see
//! [`crate::config::data_dir`]). [`crate::overlay::menu`] owns one of each; [`crate::ui`]
//! renders them.

pub mod bookmarks;
pub mod dial;
pub mod downloads;
pub mod history;

use crate::config;
use serde::{de::DeserializeOwned, Serialize};

/// Full path of a data file (`<file>`) in the user data dir.
fn data_path(file: &str) -> String {
    format!("{}{file}", config::data_dir())
}

/// Load a TOML store from `<file>` in the user data dir, falling back to
/// `T::default()` if the file is missing or unparseable (graceful degradation of
/// a hand-edited file). Each store passes its own typed `Store` wrapper as `T`.
fn load_toml<T: DeserializeOwned + Default>(file: &str) -> T {
    std::fs::read_to_string(data_path(file))
        .ok()
        .and_then(|text| toml::from_str::<T>(&text).ok())
        .unwrap_or_default()
}

/// Best-effort persist of a TOML store to `<file>`; failures are logged with the
/// human label `what`, not fatal. Returns whether the write succeeded so callers
/// that defer disk writes (e.g. history's dirty flag) can retry on failure.
fn save_toml<T: Serialize>(file: &str, val: &T, what: &str) -> bool {
    let text = match toml::to_string_pretty(val) {
        Ok(text) => text,
        Err(e) => {
            log::warn!("could not serialize {what}: {e}");
            return false;
        }
    };
    match std::fs::write(data_path(file), text) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("could not write {what}: {e}");
            false
        }
    }
}

/// Highlighted row in a menu list, shared by the bookmarks / history / downloads
/// stores. `reserved` is the count of leading non-entry rows the list shows
/// before its entries (0 for bookmarks/downloads; 1 for history, whose row 0 is
/// "Clear all"), so the same arithmetic serves both plain and offset lists. The
/// entry count is passed per call rather than stored — the owning store holds the
/// data.
struct ListCursor {
    selected: usize,
    reserved: usize,
}

impl ListCursor {
    /// A cursor over a list with `reserved` leading non-entry rows (0 for a plain
    /// list).
    fn new(reserved: usize) -> Self {
        Self {
            selected: 0,
            reserved,
        }
    }

    fn selected(&self) -> usize {
        self.selected
    }

    /// Reset the highlight to the first entry (past the reserved rows), or 0 when
    /// the list is empty.
    fn reset(&mut self, len: usize) {
        self.selected = if len == 0 { 0 } else { self.reserved };
    }

    /// Move the highlight by `dy` rows, clamped across the reserved rows plus the
    /// `len` entries. No-op on an empty list.
    fn move_sel(&mut self, dy: i32, len: usize) {
        if len == 0 {
            return;
        }
        let last = (len + self.reserved) as i32 - 1;
        self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
    }

    /// Clamp the highlight after the list shrank to `len` entries.
    fn clamp(&mut self, len: usize) {
        self.selected = self.selected.min((len + self.reserved).saturating_sub(1));
    }

    /// Index into the entry list for the highlighted row, or `None` when a
    /// reserved row (e.g. "Clear all") is highlighted.
    fn entry_index(&self) -> Option<usize> {
        self.selected.checked_sub(self.reserved)
    }

    /// Whether a reserved leading row is highlighted. Only meaningful on a
    /// non-empty list (the rows aren't shown otherwise).
    fn on_reserved_row(&self, len: usize) -> bool {
        self.reserved != 0 && len != 0 && self.selected < self.reserved
    }
}
