//! Game Mode's slice of the chrome: the screens that shrink the browser's
//! vocabulary, the entry toast's wording, and the map name the Gaming tab shows.

use super::{drop_egui_focus, AppUi};
use std::time::Duration;

/// How long the Game Mode entry toast stays before fading.
const GAME_MODE_TOAST: Duration = Duration::from_secs(4);

impl AppUi {
    /// Whether one of Game Mode's own screens is up. The browser's vocabulary
    /// shrinks under any of them, in or out of the mode. Visibility, not
    /// [`Focus::is_game_screen`]: it stays true with the OSK open over one.
    #[inline]
    pub fn game_screen(&self) -> bool {
        self.quick_access.visible || self.input_maps.visible() || self.map_edit.visible()
    }

    /// Enter Game Mode, showing `toast` (worded by [`game_mode_toast_text`]).
    /// Dropping egui's keyboard focus is part of it: egui is offered every key
    /// before we are, so a focused address bar would go on eating them.
    pub fn enter_game_mode(&mut self, toast: String) {
        self.toast_for(toast, GAME_MODE_TOAST);
        drop_egui_focus(&self.egui_ctx);
    }

    pub fn leave_game_mode(&mut self) {
        self.toast = None;
    }

    /// Adopt the name of a map chosen in the menu, or set by an edited
    /// config.
    #[inline]
    pub fn set_input_map_name(&mut self, name: String) {
        self.input_map_name = name;
    }
}

/// Word the entry toast from the ways to the menu this device has: the gesture
/// the pad reserves, and whatever `game_mode` answers to on a keyboard. Leaving
/// is a row in that menu, so the toast only has to point at it.
pub fn game_mode_toast_text(pad: Option<String>, keys: &[String]) -> String {
    let mut ways: Vec<&str> = pad.iter().map(String::as_str).collect();
    ways.extend(keys.iter().map(String::as_str));
    let mut text = "Game Mode".to_string();
    if !ways.is_empty() {
        text.push_str(" - ");
        text.push_str(&ways.join(" / "));
        text.push_str(" for Quick Access");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::game_mode_toast_text;

    /// The toast is the only thing on screen once the chrome hides, so it has to
    /// name the gestures this install really has — not the defaults.
    #[test]
    fn the_toast_names_the_bound_gestures_and_nothing_else() {
        let keys = vec!["ctrl+g".to_string()];
        let pad = || Some("hold:start".to_string());
        assert_eq!(
            game_mode_toast_text(pad(), &keys),
            "Game Mode - hold:start / ctrl+g for Quick Access"
        );
        // No pad: naming its gesture would point at a button that is not there.
        assert_eq!(
            game_mode_toast_text(None, &keys),
            "Game Mode - ctrl+g for Quick Access"
        );
        assert_eq!(
            game_mode_toast_text(pad(), &[]),
            "Game Mode - hold:start for Quick Access"
        );
        // Nothing to name leaves no dangling separator behind.
        assert_eq!(game_mode_toast_text(None, &[]), "Game Mode");
    }
}
