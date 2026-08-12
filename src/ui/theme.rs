//! Shared UI accent, icon font, and the egui visual theme. One place defines the
//! green accent so the start page, the menu, and egui's own selection highlights
//! all agree; [`apply`] installs it (and the Phosphor icon faces every chrome
//! glyph comes from) onto the egui context once at startup, so every
//! `selectable` widget (menu section bar, list rows), text selection, and link
//! picks it up without per-widget styling.

use egui_phosphor::Variant;
use egui_sdl2::egui;

/// The brand accent (teal-green) — selected/active emphasis across the UI.
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3f, 0xb8, 0xa0);

/// Dark chrome panel fill, shared by the menu / settings / prompt overlays.
pub const PANEL_FILL: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x1c);

/// Secondary / label text: hints, dates, statuses, mouse-only actions.
pub const DIM: egui::Color32 = egui::Color32::from_gray(0x99);

/// A refused action: the success teal cannot read as "this did not happen".
pub const WARN: egui::Color32 = egui::Color32::from_rgb(0xe8, 0x73, 0x5a);

/// Row / section-bar font size shared across the full-screen overlays.
pub const ROW_FONT: f32 = 15.0;

/// The Phosphor weight the chrome icons are drawn in. Bold holds up at the icon
/// sizes a handheld runs; the lighter weights wash out under 20px.
const ICON_VARIANT: Variant = Variant::Bold;

/// Family holding Phosphor's filled weight — the "on" half of an icon pair (see
/// [`icon_fill`]). A named family, not another fallback in `Proportional`, so the
/// solid glyphs stay opt-in per widget instead of shadowing the outlined ones
/// (both weights map a glyph to the same code point).
const FILL_FAMILY: &str = "phosphor-fill";

/// Icon size for buttons and rows. Phosphor draws its artwork inset in the em
/// box, so an icon needs a few points over the body text to match it optically.
pub const ICON_SIZE: f32 = 15.0;

/// Install the icon font and the accent on egui's dark theme: a translucent
/// accent fill behind selected widgets (so white text stays readable over the
/// dark panels) ringed by the solid accent, plus accent-colored links and text
/// caret.
pub fn apply(ctx: &egui::Context) {
    // Chrome icons are Phosphor glyphs: egui's bundled fonts cover only a
    // scattered subset of icon-ish Unicode, in mismatched weights, and every
    // addition had to be cmap-checked against three faces first.
    //
    // `add_to_fonts` puts Phosphor *second* in the Proportional family. Both
    // neighbours matter: Ubuntu-Light stays first, so row metrics are unchanged
    // (epaint reads a family's metrics from its first face only), and the two
    // emoji faces stay behind it, so Phosphor wins the private-use range they
    // also map.
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, ICON_VARIANT);
    fonts
        .font_data
        .insert(FILL_FAMILY.to_owned(), Variant::Fill.font_data().into());
    // Ubuntu-Light leads this family too, so a filled icon lays out identically
    // to the outlined one it swaps with.
    fonts.families.insert(
        egui::FontFamily::Name(FILL_FAMILY.into()),
        vec!["Ubuntu-Light".to_owned(), FILL_FAMILY.to_owned()],
    );
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

/// An icon glyph (one of [`egui_phosphor::bold`]'s constants) as widget text at
/// [`ICON_SIZE`]. Callers add color like any other [`egui::RichText`].
pub fn icon(glyph: &str) -> egui::RichText {
    egui::RichText::new(glyph).size(ICON_SIZE)
}

/// The filled counterpart of [`icon`] — the same glyph in Phosphor's solid
/// weight, for the "on" half of a pair (a saved bookmark against an unsaved
/// one). Takes the matching [`egui_phosphor::fill`] constant.
pub fn icon_fill(glyph: &str) -> egui::RichText {
    egui::RichText::new(glyph)
        .size(ICON_SIZE)
        .family(egui::FontFamily::Name(FILL_FAMILY.into()))
}

/// Side of the square close button (logical px).
pub const CLOSE_SIZE: f32 = 28.0;

/// A mouse-only close button drawn at `rect`: a rounded outline with a centered
/// X, both brightening to the accent on hover. Shared by the full-screen
/// overlays (the menu and the dial editor) — a gamepad closes them with B
/// instead. Returns the click response. `id` must be unique per call site (two
/// overlays can be on screen at once).
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
        egui_phosphor::bold::X,
        egui::FontId::proportional(ICON_SIZE),
        ink,
    );
    resp
}
