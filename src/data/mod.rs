//! User data stores, all shaped alike: an in-memory list with a highlighted row
//! for the menu, persisted as TOML in the user data dir (see
//! [`crate::config::data_dir`]). [`crate::overlay::menu`] owns one of each; [`crate::ui`]
//! renders them.

pub mod bookmarks;
pub mod downloads;
pub mod history;
pub mod page_icons;
pub mod session;

use crate::config;
use crate::list::ListCursor;
use serde::{de::DeserializeOwned, Serialize};

/// Full path of a data file (`<file>`) in the user data dir.
pub(super) fn data_path(file: &str) -> String {
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
    match write_atomic(&data_path(file), &text) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("could not write {what}: {e}");
            false
        }
    }
}

/// Write `text` to `path` through a temp file and a rename, so a crash or a
/// full card mid-write leaves the old file whole.
pub(crate) fn write_atomic(path: &str, text: &str) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// A file stem for a typed name: lowercased, one dash per run of anything
/// else, `fallback` for a name with nothing to keep, never one of `taken`.
pub(crate) fn new_id(name: &str, taken: &[String], fallback: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| match c.is_alphanumeric() {
            true => c.to_lowercase().next().unwrap_or(c),
            false => '-',
        })
        .collect();
    let base: String = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = match base.is_empty() {
        true => fallback.to_string(),
        false => base,
    };
    if !taken.contains(&base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Best-effort delete of a data file (a store whose feature was turned off); a
/// missing file counts as done, other failures are logged as `what`.
fn remove(file: &str, what: &str) {
    match std::fs::remove_file(data_path(file)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("could not remove {what}: {e}"),
    }
}
