//! Visit history: a capped, most-recent-first list of visited URLs (each with a
//! visit timestamp) persisted to `history.toml` in the user data dir. Recording
//! can be disabled and the cap configured via `[history]` in the config file (see
//! [`crate::config`]). The full-screen menu (see [`crate::overlay::menu`]) renders it; the
//! central router drives selection / open / delete / clear.

use crate::config::{self, HistoryConfig};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A visited page and when it was last visited (unix seconds, `0` if the device
/// clock was unavailable — common on handhelds without an RTC until NTP syncs).
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub url: String,
    #[serde(default)]
    pub time: u64,
}

/// On-disk shape (a TOML table can't be a bare array, so wrap the list).
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    entries: Vec<Entry>,
}

pub struct History {
    /// Visited entries, most-recent first.
    entries: Vec<Entry>,
    /// Whether new visits are recorded (existing entries are still shown/cleared).
    enabled: bool,
    /// Cap on retained entries; oldest are dropped past this.
    max_entries: usize,
    /// Highlighted row in the menu's History section.
    selected: usize,
}

impl History {
    /// Load the saved list (missing/invalid file → empty), trimmed to the cap.
    pub fn load(cfg: &HistoryConfig) -> Self {
        let mut entries = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| toml::from_str::<Store>(&text).ok())
            .map(|store| store.entries)
            .unwrap_or_default();
        // Honor a shrunk cap from the config right away.
        entries.truncate(cfg.max_entries);
        Self {
            entries,
            enabled: cfg.enabled,
            max_entries: cfg.max_entries,
            selected: 0,
        }
    }

    fn path() -> String {
        format!("{}history.toml", config::data_dir())
    }

    /// Best-effort persist; failures are logged, not fatal.
    fn save(&self) {
        let store = Store {
            entries: self.entries.clone(),
        };
        match toml::to_string_pretty(&store) {
            Ok(text) => {
                if let Err(e) = std::fs::write(Self::path(), text) {
                    log::warn!("could not write history: {e}");
                }
            }
            Err(e) => log::warn!("could not serialize history: {e}"),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The highlighted row in the menu's History section. Index 0 is the
    /// "Clear all" top row; the entries follow at `1..=entries.len()`.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Whether the "Clear all" top row (cursor index 0) is highlighted. Only
    /// meaningful while there are entries (the row isn't shown otherwise).
    pub fn clear_selected(&self) -> bool {
        !self.entries.is_empty() && self.selected == 0
    }

    /// Record a visit: most-recent-first, de-duplicated (a revisit moves to the
    /// top and re-stamps its time), capped at `max_entries`. No-op when recording
    /// is disabled or the URL is empty. Persists on change.
    pub fn record(&mut self, url: &str) {
        if !self.enabled || url.is_empty() {
            return;
        }
        // Already on top → just keep the existing entry (avoids rewriting the file
        // every frame for the page we're currently on).
        if self.entries.first().is_some_and(|e| e.url == url) {
            return;
        }
        if let Some(i) = self.entries.iter().position(|e| e.url == url) {
            self.entries.remove(i);
        }
        self.entries.insert(
            0,
            Entry {
                url: url.to_string(),
                time: now_unix(),
            },
        );
        self.entries.truncate(self.max_entries);
        self.clamp_selected();
        self.save();
    }

    /// Reset the highlight when the menu opens: land on the first entry (index
    /// 1), not the destructive "Clear all" row at index 0 — mirrors how the Tabs
    /// section starts on the first tab rather than "+ New tab".
    pub fn reset(&mut self) {
        self.selected = usize::from(!self.entries.is_empty());
    }

    /// Move the highlight by `dy` rows, clamped to the list. Index 0 is the
    /// "Clear all" row; entries occupy `1..=entries.len()`.
    pub fn move_sel(&mut self, dy: i32) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() as i32; // 0 = Clear all, 1..=len = entries
        self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
    }

    pub fn selected_url(&self) -> Option<String> {
        // Index 0 is the "Clear all" row (no URL); entries start at 1.
        self.selected
            .checked_sub(1)
            .and_then(|i| self.entries.get(i))
            .map(|e| e.url.clone())
    }

    /// Remove the highlighted entry; persists. No-op on the "Clear all" row.
    pub fn remove_selected(&mut self) {
        if let Some(i) = self.selected.checked_sub(1) {
            self.remove(i);
        }
    }

    /// Remove the entry at `index` (if in range); persists.
    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            self.clamp_selected();
            self.save();
        }
    }

    /// Drop every entry; persists.
    pub fn clear(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.entries.clear();
        self.selected = 0;
        self.save();
    }

    fn clamp_selected(&mut self) {
        // Max cursor index is `entries.len()` — index 0 is "Clear all", then the
        // entries at `1..=entries.len()`.
        self.selected = self.selected.min(self.entries.len());
    }
}

/// Current unix time in seconds, or `0` if the clock is before the epoch / broken.
pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format a unix timestamp as a compact `YYYY-MM-DD HH:MM` in UTC, dependency-free
/// (Howard Hinnant's civil-from-days). Returns `—` for an unknown (`0`) time so a
/// device with no working clock doesn't show a misleading `1970` date.
pub fn format_time(secs: u64) -> String {
    if secs == 0 {
        return "—".to_string();
    }
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = tod / 3_600;
    let min = (tod % 3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}")
}

/// Convert days since the unix epoch to a `(year, month, day)` civil date (UTC).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}
