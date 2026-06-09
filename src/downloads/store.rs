//! Persistence of the download list: finished entries (done or failed) go to
//! `downloads.toml` in the user data dir, mirroring `bookmarks.toml` /
//! `history.toml`. Active downloads are never written — they can't be resumed
//! across a restart, so they simply vanish from the list.

use super::{Download, State};
use crate::config;
use serde::{Deserialize, Serialize};

/// On-disk shape of a finished download.
#[derive(Serialize, Deserialize)]
struct DiskEntry {
    url: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    time: u64,
    /// `None` for a successful download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// On-disk shape (a TOML table can't be a bare array, so wrap the list).
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    entries: Vec<DiskEntry>,
}

fn path() -> String {
    format!("{}downloads.toml", config::data_dir())
}

/// Load the saved entries (missing/invalid file → empty).
pub(super) fn load() -> Vec<Download> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| toml::from_str::<Store>(&text).ok())
        .map(|store| store.entries.into_iter().map(into_download).collect())
        .unwrap_or_default()
}

/// Best-effort persist of the finished entries; failures are logged, not fatal.
pub(super) fn save(items: &[Download]) {
    let store = Store {
        entries: items
            .iter()
            .filter(|d| !d.is_active())
            .map(|d| DiskEntry {
                url: d.url.clone(),
                path: d.path.clone(),
                size: d.received,
                time: d.time,
                error: match &d.state {
                    State::Failed(e) => Some(e.clone()),
                    _ => None,
                },
            })
            .collect(),
    };
    match toml::to_string_pretty(&store) {
        Ok(text) => {
            if let Err(e) = std::fs::write(path(), text) {
                log::warn!("could not write downloads: {e}");
            }
        }
        Err(e) => log::warn!("could not serialize downloads: {e}"),
    }
}

fn into_download(entry: DiskEntry) -> Download {
    Download {
        filename: super::file_name_of(&entry.path),
        url: entry.url,
        received: entry.size,
        total: entry.size,
        time: entry.time,
        state: match entry.error {
            Some(e) => State::Failed(e),
            None => State::Done,
        },
        path: entry.path,
        shared: None,
    }
}
