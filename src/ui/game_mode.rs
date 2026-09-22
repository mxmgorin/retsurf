//! Game Mode's slice of the chrome: the mode flag the router and keyboard
//! read, the entry toast, and the map name its menu shows.

use super::{drop_egui_focus, AppUi};
use egui_sdl2::egui;
use std::time::{Duration, Instant};

/// How long the Game Mode entry toast stays before fading.
const GAME_MODE_TOAST: Duration = Duration::from_secs(4);

impl AppUi {
    #[inline]
    pub fn game_mode(&self) -> bool {
        self.game_mode
    }

    /// Whether one of Game Mode's own screens is up. The browser's vocabulary
    /// shrinks under any of them, in or out of the mode. Visibility, not
    /// [`Focus::is_game_screen`]: it stays true with the OSK open over one.
    #[inline]
    pub fn game_screen(&self) -> bool {
        self.game_menu.visible || self.input_maps.visible() || self.map_edit.visible()
    }

    /// Enter Game Mode, showing `toast` (worded by [`game_mode_toast_text`]).
    /// Dropping egui's keyboard focus is part of it: egui is offered every key
    /// before we are, so a focused address bar would go on eating them.
    pub fn enter_game_mode(&mut self, toast: String) {
        self.game_mode = true;
        self.game_mode_toast = Some(Instant::now());
        self.game_mode_toast_text = toast;
        drop_egui_focus(&self.egui_ctx);
    }

    pub fn leave_game_mode(&mut self) {
        self.game_mode = false;
        self.game_mode_toast = None;
    }

    /// Adopt the name of a map chosen in the menu, or set by an edited
    /// config.
    #[inline]
    pub fn set_input_map_name(&mut self, name: String) {
        self.input_map_name = name;
    }

    /// Time left on the entry toast, or `None` once it has faded.
    pub(super) fn toast_visible_for(&self) -> Option<Duration> {
        self.game_mode_toast
            .and_then(|t| GAME_MODE_TOAST.checked_sub(t.elapsed()))
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
        text.push_str(" for the menu");
    }
    text
}

/// The Game Mode entry toast: the chrome just hid, so name the way back.
pub(super) fn add_game_mode_toast(ctx: &egui::Context, text: &str) {
    egui::Area::new(egui::Id::new("game_mode_toast"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(text);
            });
        });
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
            "Game Mode - hold:start / ctrl+g for the menu"
        );
        // No pad: naming its gesture would point at a button that is not there.
        assert_eq!(
            game_mode_toast_text(None, &keys),
            "Game Mode - ctrl+g for the menu"
        );
        assert_eq!(
            game_mode_toast_text(pad(), &[]),
            "Game Mode - hold:start for the menu"
        );
        // Nothing to name leaves no dangling separator behind.
        assert_eq!(game_mode_toast_text(None, &[]), "Game Mode");
    }
}
