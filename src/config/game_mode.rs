use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// Game Mode (`[game_mode]`): the browser stops consuming input so the page
/// gets it. There is no enable switch — entering is the `game_mode` binding.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameModeConfig {
    /// How the pad reaches the game while Game Mode is on. Applies at startup.
    pub profile: GameProfile,
}

token_enum! {
    /// The built-in pad mapping (`[game_mode] profile`). Both keep the right
    /// stick as the cursor and R2 as the click, and reserve Select for the exit.
    pub enum GameProfile {
        default Keys;
        /// The retro-convention keyboard mapping (arrows + z/x/c + Space/Enter),
        /// for the keyboard-driven games that dominate itch.io and poki.
        Keys => "keys", "Keyboard keys",
        /// The pad reaches the page raw through the Gamepad API, for games that
        /// read it themselves.
        Pad => "pad", "Gamepad passthrough",
    }
}
