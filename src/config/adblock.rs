use serde::{Deserialize, Serialize};

/// Ad-blocker settings (`[adblock]` in the config): network-level filtering via
/// Brave's adblock-rust engine — see [`crate::browser::adblock`].
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdblockConfig {
    /// Master switch. When off, no lists are fetched and nothing is filtered.
    pub enabled: bool,
    /// Filter lists (EasyList syntax) downloaded into the engine.
    pub lists: Vec<String>,
    /// Re-download the lists once the cached engine is older than this many
    /// days; `0` never refreshes (keeps using whatever cache exists).
    pub update_days: u64,
}

impl Default for AdblockConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lists: vec![
                "https://easylist.to/easylist/easylist.txt".to_string(),
                "https://easylist.to/easylist/easyprivacy.txt".to_string(),
            ],
            update_days: 7,
        }
    }
}
