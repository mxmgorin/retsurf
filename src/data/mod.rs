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
