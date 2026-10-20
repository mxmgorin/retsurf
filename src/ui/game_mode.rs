//! Game Mode's slice of the chrome: the screens that shrink the browser's
//! vocabulary, the entry hint, and the map name the Gaming tab shows.

use super::toast::{Notice, Part};
use super::{drop_egui_focus, AppUi};
use crate::overlay::game::map_edit::pad_label;
use crate::overlay::quick_access::Strip;
use egui_phosphor::bold;
use inputbind::PadGesture;
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

    /// Enter Game Mode, showing `hint` (see [`game_mode_hint`]). Dropping
    /// egui's keyboard focus is part of it: egui is offered every key before
    /// we are, so a focused address bar would go on eating them.
    pub fn enter_game_mode(&mut self, hint: Notice) {
        self.toast_for(hint, GAME_MODE_TOAST);
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

/// The entry hint: how to reach each strip on the device last used (the
/// keyboard after a key or mouse press, `keyboard`), falling back to whichever
/// device has any gesture.
pub fn game_mode_hint(
    pad: &[(PadGesture, Strip)],
    keys: &[(String, Strip)],
    keyboard: bool,
) -> Notice {
    let pad: Vec<_> = pad
        .iter()
        .map(|&(gesture, strip)| (pad_caps(gesture), strip))
        .collect();
    let keys: Vec<_> = keys
        .iter()
        .map(|(gesture, strip)| (key_caps(gesture), *strip))
        .collect();
    let (first, second) = match keyboard {
        true => (keys, pad),
        false => (pad, keys),
    };
    let ways = if first.is_empty() { second } else { first };
    if ways.is_empty() {
        return Notice::new(bold::GAME_CONTROLLER, "Game mode");
    }
    // One verb for all when they share it: `Hold [Start] for ..., [Select] for ...`.
    let shared = ways.iter().all(|((verb, _), _)| *verb == ways[0].0 .0);
    let last = ways.len() - 1;
    let mut parts = vec![];
    for (i, ((verb, caps), strip)) in ways.into_iter().enumerate() {
        if i == 0 || !shared {
            parts.push(Part::Text(verb.to_string()));
        }
        for (j, cap) in caps.into_iter().enumerate() {
            if j > 0 {
                parts.push(Part::Text("+".to_string()));
            }
            parts.push(Part::Key(cap));
        }
        let comma = if i < last { "," } else { "" };
        parts.push(Part::Text(format!("for {}{comma}", strip.name())));
    }
    Notice::parts(bold::GAME_CONTROLLER, parts)
}

/// [`pad_caps`]'s verb for a held gesture.
const HOLD: &str = "Hold";
/// Its verb for a tap or a chord.
const PRESS: &str = "Press";

/// A pad gesture as a verb and the buttons it takes.
fn pad_caps(gesture: PadGesture) -> (&'static str, Vec<String>) {
    let (verb, pads) = match gesture {
        PadGesture::Hold(pad) => (HOLD, vec![pad]),
        PadGesture::Tap(pad) => (PRESS, vec![pad]),
        PadGesture::Chord(leader, second) => (PRESS, vec![leader, second]),
    };
    let caps = pads
        .into_iter()
        .map(|pad| pad_label(pad).to_string())
        .collect();
    (verb, caps)
}

/// A keyboard gesture as caps: `ctrl+alt+a` reads `Ctrl`, `Alt`, `A`.
fn key_caps(gesture: &str) -> (&'static str, Vec<String>) {
    let caps = gesture
        .split('+')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect();
    (PRESS, caps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use inputbind::Pad;

    fn text(text: &str) -> Part {
        Part::Text(text.to_string())
    }

    fn key(key: &str) -> Part {
        Part::Key(key.to_string())
    }

    fn hint(parts: Vec<Part>) -> Notice {
        Notice::parts(bold::GAME_CONTROLLER, parts)
    }

    /// The hint is the only thing on screen once the chrome hides, so it names
    /// the gestures this install really has, on the device last used.
    #[test]
    fn the_hint_names_the_bound_gestures_of_the_device_in_hand() {
        let keys = vec![
            ("ctrl+alt+a".to_string(), Strip::QuickAccess),
            ("ctrl+m".to_string(), Strip::QuickMenu),
        ];
        let pad = [
            (PadGesture::Hold(Pad::Start), Strip::QuickAccess),
            (PadGesture::Hold(Pad::Select), Strip::QuickMenu),
        ];
        // A shared verb is said once.
        assert_eq!(
            game_mode_hint(&pad, &keys, false),
            hint(vec![
                text("Hold"),
                key("Start"),
                text("for Quick Access,"),
                key("Select"),
                text("for Quick Menu"),
            ])
        );
        assert_eq!(
            game_mode_hint(&pad, &keys[..1], true),
            hint(vec![
                text("Press"),
                key("Ctrl"),
                text("+"),
                key("Alt"),
                text("+"),
                key("A"),
                text("for Quick Access"),
            ])
        );
        // The other device stands in for one that has nothing bound.
        assert_eq!(game_mode_hint(&pad[..1], &[], true).parts.len(), 3);
        let mixed = [
            (PadGesture::Hold(Pad::Start), Strip::QuickAccess),
            (PadGesture::Chord(Pad::Select, Pad::Y), Strip::QuickMenu),
        ];
        let parts = game_mode_hint(&mixed, &[], false).parts;
        assert_eq!(
            parts
                .iter()
                .filter(|part| matches!(part, Part::Text(t) if t == "Press"))
                .count(),
            1
        );
        assert_eq!(
            game_mode_hint(&[], &[], false),
            Notice::new(bold::GAME_CONTROLLER, "Game mode")
        );
    }
}
