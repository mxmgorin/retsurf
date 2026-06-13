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
    // egui's default Proportional family is Ubuntu → NotoEmoji → emoji-icon and
    // never consults Hack (the Monospace face). Some toolbar glyphs — the plain
    // arrows ← → among them — live *only* in Hack, so without this they render as
    // tofu. Append Hack as a last-resort fallback: Ubuntu is still tried first, so
    // ordinary text is unchanged; Hack only fills glyphs the others lack.
    let mut fonts = egui::FontDefinitions::default();
    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.push("Hack".to_owned());
    }
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    // Selected `selectable` widgets and highlighted text: a low-alpha accent
    // wash tints the row without swamping the foreground text, ringed crisply.
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.30);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.text_cursor.stroke.color = ACCENT;
    ctx.set_visuals(visuals);
}

/// Side of the square ✖ close button (logical px).
pub const CLOSE_SIZE: f32 = 28.0;

/// A mouse-only ✖ close button drawn at `rect`: a rounded outline with a
/// centered ✖, both brightening to the accent on hover. Shared by the
/// full-screen overlays (the menu and the dial editor) — a gamepad closes them
/// with B instead. Returns the click response. `id` must be unique per call site
/// (two overlays can be on screen at once).
pub fn close_button(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id) -> egui::Response {
    let resp = ui.interact(rect, id, egui::Sense::click());
    let hot = resp.hovered();
    let line = if hot {
        ACCENT
    } else {
        egui::Color32::from_gray(0x44)
    };
    let ink = if hot {
        ACCENT
    } else {
        egui::Color32::from_gray(0xe0)
    };
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.0, line),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "✖",
        egui::FontId::proportional(15.0),
        ink,
    );
    resp
}
