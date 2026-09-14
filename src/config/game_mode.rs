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
    /// stick as the cursor and R2 as the click, and reserve Select for the menu.
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

impl GameProfile {
    /// This profile's human label — the Game Mode menu's Profile row value.
    pub fn label(self) -> &'static str {
        Self::CHOICES
            .iter()
            .find(|(_, token)| *token == self.as_str())
            .map(|(label, _)| *label)
            .expect("CHOICES and as_str are generated from one table")
    }

    /// The profile `delta` steps along [`Self::CHOICES`], wrapping — how the
    /// menu switches it without leaving Game Mode.
    pub fn cycle(self, delta: i32) -> Self {
        let choices = Self::CHOICES;
        let at = choices
            .iter()
            .position(|(_, token)| *token == self.as_str())
            .expect("CHOICES and as_str are generated from one table");
        let next = (at as i32 + delta).rem_euclid(choices.len() as i32) as usize;
        Self::from_value(choices[next].1)
    }
}

#[cfg(test)]
mod tests {
    use super::GameProfile;

    #[test]
    fn cycling_wraps_in_both_directions_and_labels_every_profile() {
        let mut seen = Vec::new();
        let mut profile = GameProfile::default();
        for _ in 0..GameProfile::CHOICES.len() {
            assert!(!profile.label().is_empty());
            seen.push(profile);
            profile = profile.cycle(1);
        }
        // A full lap comes home, and the other direction is its mirror.
        assert_eq!(profile, GameProfile::default());
        assert_eq!(profile.cycle(-1), seen[seen.len() - 1]);
    }
}
