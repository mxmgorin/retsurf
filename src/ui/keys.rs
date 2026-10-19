//! Buttons and keys named on screen, each drawn as a cap so it reads apart from
//! the words around it, and the hint rows built from them.

use super::panel::ROW_RADIUS;
use super::theme::{BORDER, DIM, INK, SURFACE};
use egui_sdl2::egui;

/// Type size of a hint row.
pub(super) const HINT_FONT: f32 = 13.0;
/// Space between one hint and the next.
const HINT_GAP: f32 = 12.0;
/// Space between a cap and what it is next to.
const CAP_GAP: f32 = 4.0;

/// One button or key, framed like a key, its label at `size`.
pub(super) fn key_cap(ui: &mut egui::Ui, key: &str, size: f32) {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(ROW_RADIUS)
        .inner_margin(egui::Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(key).color(INK).size(size).strong());
        });
}

/// The D-pad's axes as one badge each: up with down, then left with right.
pub(super) fn dpad_caps() -> (String, String) {
    use egui_phosphor::bold::{CARET_DOWN, CARET_LEFT, CARET_RIGHT, CARET_UP};
    (
        format!("{CARET_UP}{CARET_DOWN}"),
        format!("{CARET_LEFT}{CARET_RIGHT}"),
    )
}

/// A hint: the caps of what does it, then what it does. No caps is a note.
pub(super) type Hint<'a> = (&'a [&'a str], &'a str);

/// A row of hints, wrapping where the width runs out.
pub(super) fn hint_row(ui: &mut egui::Ui, hints: &[Hint]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(CAP_GAP, CAP_GAP);
        for (i, (keys, action)) in hints.iter().enumerate() {
            if i > 0 {
                ui.add_space(HINT_GAP - CAP_GAP);
            }
            for key in *keys {
                key_cap(ui, key, HINT_FONT);
            }
            ui.label(egui::RichText::new(*action).color(DIM).size(HINT_FONT));
        }
    });
}
