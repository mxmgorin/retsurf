use serde::{Deserialize, Serialize};

/// On-screen-keyboard settings (`[osk]` in the config): which of the built-in
/// layouts are enabled — see [`crate::overlay::osk`] for the layout data itself.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OskConfig {
    /// Enabled layouts, in the order the keyboard's Lang key cycles them.
    /// Unknown names are logged and skipped; an empty (or fully invalid) list
    /// falls back to `["en"]`, so the keyboard always works.
    pub layouts: Vec<String>,
}

impl Default for OskConfig {
    fn default() -> Self {
        Self {
            layouts: vec!["en".to_string(), "ru".to_string()],
        }
    }
}
