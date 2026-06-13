//! Shared UI accent and the egui visual theme. One place defines the green
//! accent so the start page, the menu, and egui's own selection highlights all
//! agree; [`apply`] installs it onto the egui context once at startup so every
//! `selectable` widget (menu section bar, list rows), text selection, and link
//! picks it up without per-widget styling.

use egui_sdl2::egui;

/// The brand accent (teal-green) — selected/active emphasis across the UI.
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3f, 0xb8, 0xa0);

/// Install the accent on egui's dark theme: a translucent accent fill behind
/// selected widgets (so white text stays readable over the dark panels) ringed
/// by the solid accent, plus accent-colored links and text caret.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    // Selected `selectable` widgets and highlighted text: a low-alpha accent
    // wash tints the row without swamping the foreground text, ringed crisply.
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.30);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.text_cursor.stroke.color = ACCENT;
    ctx.set_visuals(visuals);
}
