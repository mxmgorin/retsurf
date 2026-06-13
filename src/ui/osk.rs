//! Rendering of the on-screen keyboard (state and input handling live in
//! [`crate::overlay::osk`]).

use super::theme::ACCENT;
use crate::overlay::osk::{Key, Osk};
use egui_sdl2::egui;

/// The gamepad button that directly triggers a key, shown as a corner badge so
/// the shortcuts are discoverable. Mirrors the bindings in [`crate::overlay::osk`]
/// (and the router); keys without a dedicated button use D-pad + **A**.
fn button_hint(key: &Key) -> Option<&'static str> {
    match key {
        Key::Backspace => Some("X"),
        Key::Space => Some("Y"),
        Key::Shift => Some("L2"),
        Key::Enter => Some("R2"),
        Key::Hide => Some("B"),
        _ => None,
    }
}

/// Draw the on-screen keyboard, Steam-Deck style: a dark rounded overlay anchored
/// to the bottom, with the selected key (and active Shift/Caps) highlighted.
pub(super) fn add_osk(ctx: &egui::Context, osk: &Osk) {
    let selected = osk.selected();
    let shift = osk.shift();
    let highlight = ACCENT;
    let key_fill = egui::Color32::from_rgb(0x3a, 0x3a, 0x40);
    let hint = egui::Color32::from_gray(150);
    // Char keys are 36 wide with 4px gaps, so the 14-key top rows span 574px
    // (≈598 with the frame margin, inside the 640px window). Enter and Shift are
    // sized to make their (shorter) rows fill that same width.
    let key_width = |key: &Key| match key {
        Key::Space => 298.0,
        Key::Shift => 85.0,
        Key::Enter => 76.0,
        Key::Tab | Key::Caps | Key::Backspace | Key::Lang | Key::Hide => 54.0,
        _ => 36.0,
    };

    egui::Area::new(egui::Id::new("osk"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -10.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_rgba_unmultiplied(0x18, 0x18, 0x1c, 245))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 5.0);
                    for (r, row) in osk.layout().keys().iter().enumerate() {
                        ui.horizontal(|ui| {
                            for (c, key) in row.iter().enumerate() {
                                let is_sel = (r, c) == selected;
                                let active = is_sel
                                    || (*key == Key::Shift && shift)
                                    || (*key == Key::Caps && osk.caps);
                                let size = egui::vec2(key_width(key), 38.0);
                                let fill = if active { highlight } else { key_fill };
                                let button = egui::Button::new(
                                    egui::RichText::new(osk.key_label(*key))
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(fill)
                                .corner_radius(6.0)
                                .min_size(size);
                                let response = ui.add(button);
                                // Physical-keyboard style: the shifted symbol
                                // sits small and dim in the top-right corner
                                // (letters skip it — case is obvious), and the
                                // two swap while Shift is in effect.
                                if let Key::Char(ch) = key {
                                    let main = osk.layout().resolve_char(*ch, shift, osk.caps);
                                    let alt = osk.layout().resolve_char(*ch, !shift, osk.caps);
                                    if !ch.is_alphabetic() && alt != main {
                                        ui.painter().text(
                                            response.rect.right_top() + egui::vec2(-4.0, 2.0),
                                            egui::Align2::RIGHT_TOP,
                                            alt,
                                            egui::FontId::proportional(10.0),
                                            hint,
                                        );
                                    }
                                }
                                // The keys with a direct gamepad shortcut wear it
                                // as a small badge in the top-left corner.
                                if let Some(btn) = button_hint(key) {
                                    ui.painter().text(
                                        response.rect.left_top() + egui::vec2(4.0, 2.0),
                                        egui::Align2::LEFT_TOP,
                                        btn,
                                        egui::FontId::proportional(10.0),
                                        hint,
                                    );
                                }
                            }
                        });
                    }
                });
        });
}
