use serde::{Deserialize, Serialize};

/// Visit-history settings. Recording can be turned off entirely, and the cap on
/// how many entries are kept is configurable, both via `[history]` in the config.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Whether visited pages are recorded. When false, any existing history is
    /// still shown and can be cleared, but no new entries are added.
    pub enabled: bool,
    /// Maximum entries kept (most-recent-first); older ones are dropped past this.
    pub max_entries: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 25,
        }
    }
}
