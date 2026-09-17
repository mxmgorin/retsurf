use serde::{Deserialize, Serialize};

/// Game Mode (`[game_mode]`): the browser stops consuming input so the page
/// gets it. There is no enable switch — the mode's menu is the way in.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GameModeConfig {
    /// Which input map drives the pad and the keyboard while the mode is on: a
    /// built-in's id, or the stem of an `input_maps/<id>.toml` in the data dir
    /// (see [`crate::event::game::input_map`]). Applies at startup; the mode's
    /// menu writes a choice back here.
    pub input_map: String,
}

/// The built-in the menu leads with, and what an unknown id falls back to.
pub const DEFAULT_INPUT_MAP: &str = "keys";

impl Default for GameModeConfig {
    fn default() -> Self {
        Self {
            input_map: DEFAULT_INPUT_MAP.to_string(),
        }
    }
}
