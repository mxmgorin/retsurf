use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// Game Mode (`[game_mode]`): the browser stops consuming input so the page
/// gets it. There is no enable switch — Quick Access is the way in.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GameModeConfig {
    /// Which input map drives the pad and the keyboard while the mode is on: a
    /// built-in's id, the stem of an `input_maps/<id>.toml` in the data dir (see
    /// [`crate::event::game::input_map`]), or `none`, which an unknown id is too.
    pub input_map: String,
    /// How the page's game is shown while the mode is on (`[game_mode.view]`).
    pub view: GameViewConfig,
}

/// Everything visual about a game (`[game_mode.view]`).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameViewConfig {
    /// How the game is cut out of its page and sized to the screen (see
    /// [`crate::browser::AppBrowser::set_game_scaling`]).
    pub scaling: Scaling,
}

token_enum! {
    /// How Game Mode sizes the page's game to the screen, cut out over a black
    /// backdrop; `Off` leaves the page. The labels name what is on screen.
    pub enum Scaling {
        default Off;
        Off => "off", "Page",
        /// The largest size that keeps the game's aspect ratio.
        Fit => "fit", "Game (fit)",
        /// The largest whole multiple of the game's pixels, drawn unsmoothed.
        Integer => "integer", "Game (integer)",
        /// The whole screen, aspect ratio ignored.
        Stretch => "stretch", "Game (stretch)",
    }
}

/// The map a fresh config runs.
pub const DEFAULT_INPUT_MAP: &str = "keys";

impl Default for GameModeConfig {
    fn default() -> Self {
        Self {
            input_map: DEFAULT_INPUT_MAP.to_string(),
            view: GameViewConfig::default(),
        }
    }
}
