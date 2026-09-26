use crate::config::token_enum::token_enum;
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
    /// How Game Mode sizes the page's game to the screen, cut out of the page
    /// over a black backdrop; `Off` leaves the page as it is. The labels name
    /// what is on screen, since the menu row asks "View".
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

impl Scaling {
    /// The value `dir` places along [`Self::CHOICES`], wrapping at the ends.
    pub fn step(self, dir: i32) -> Self {
        let len = Self::CHOICES.len() as i32;
        let at = Self::CHOICES
            .iter()
            .position(|(_, token)| *token == self.as_str())
            .expect("CHOICES lists every variant") as i32;
        Self::from_value(Self::CHOICES[(at + dir).rem_euclid(len) as usize].1)
    }

    pub fn label(self) -> &'static str {
        Self::CHOICES
            .iter()
            .find(|(_, token)| *token == self.as_str())
            .map(|(label, _)| *label)
            .expect("CHOICES lists every variant")
    }
}

/// The built-in the menu leads with, and what an unknown id falls back to.
pub const DEFAULT_INPUT_MAP: &str = "keys";

impl Default for GameModeConfig {
    fn default() -> Self {
        Self {
            input_map: DEFAULT_INPUT_MAP.to_string(),
            view: GameViewConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Scaling;

    #[test]
    fn the_scaling_steps_both_ways_and_wraps() {
        assert_eq!(Scaling::Off.step(1), Scaling::Fit);
        assert_eq!(Scaling::Stretch.step(1), Scaling::Off);
        assert_eq!(Scaling::Off.step(-1), Scaling::Stretch);
        assert_eq!(Scaling::Integer.label(), "Game (integer)");
    }
}
