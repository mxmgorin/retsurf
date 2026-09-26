//! Shared UI accent, icon font, and the egui visual theme. [`apply`] installs
//! them on the egui context once at startup, so every `selectable` widget, text
//! selection and link picks the accent up without per-widget styling.

use egui_phosphor::Variant;
use egui_sdl2::egui;

/// The brand accent (teal-green) — selected/active emphasis across the UI.
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3f, 0xb8, 0xa0);

/// Dark chrome panel fill, shared by the menu / settings / prompt overlays.
pub const PANEL_FILL: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x1c);

/// Secondary / label text: hints, dates, statuses, mouse-only actions.
pub const DIM: egui::Color32 = egui::Color32::from_gray(0x99);

/// Backdrop under something that blocks: what it covers reads as inactive.
pub const SCRIM: egui::Color32 = egui::Color32::from_black_alpha(140);

/// A refused action: the success teal cannot read as "this did not happen".
pub const WARN: egui::Color32 = egui::Color32::from_rgb(0xe8, 0x73, 0x5a);

// The start-page palette, shared by the dial editor (its tiles must match).
pub const BG: egui::Color32 = egui::Color32::from_rgb(0x16, 0x17, 0x1a);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x20, 0x24);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2d, 0x33);
pub const INK: egui::Color32 = egui::Color32::from_rgb(0xec, 0xec, 0xea);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(0x8a, 0x8f, 0x98);
/// The wordmark gradient's warm (coral) end, paired with [`ACCENT`].
pub const SURF_WARM: egui::Color32 = egui::Color32::from_rgb(0xff, 0x8c, 0x69);

/// Hairline ring around a floating card, and the memory overlay's border.
pub const HAIRLINE: egui::Color32 = egui::Color32::from_gray(0x55);

/// The centered floating card (modal prompt, game menu): one frame, so the two
/// read as the same surface.
pub fn card_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(PANEL_FILL)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(10.0)
        .inner_margin(14.0)
}

/// Row / section-bar font size shared across the full-screen overlays.
pub const ROW_FONT: f32 = 15.0;

/// The Phosphor weight the chrome icons are drawn in. Bold holds up at the icon
/// sizes a handheld runs; the lighter weights wash out under 20px.
const ICON_VARIANT: Variant = Variant::Bold;

/// Family holding Phosphor's filled weight (see [`icon_fill`]). A named family,
/// not another fallback in `Proportional`: both weights map a glyph to the same
/// code point, so the solid one would shadow the outlined one.
const FILL_FAMILY: &str = "phosphor-fill";

/// Icon size for buttons and rows. Phosphor draws its artwork inset in the em
/// box, so an icon needs a few points over the body text to match it optically.
pub const ICON_SIZE: f32 = 15.0;

/// Install the icon font and the accent on egui's dark theme: a translucent
/// accent fill behind selected widgets, ringed by the solid accent, plus
/// accent-colored links and caret.
pub fn apply(ctx: &egui::Context) {
    // `add_to_fonts` puts Phosphor *second*: Ubuntu-Light keeps the family's
    // metrics, and Phosphor still wins the private-use range the emoji faces map.
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
    // A low-alpha wash tints the row without swamping the text over it.
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.30);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.text_cursor.stroke.color = ACCENT;
    // A blink asks for a frame twice a second, and without a GPU that is a full
    // software rasterization of the chrome (80 ms on a Miyoo).
    visuals.text_cursor.blink = false;
    ctx.set_visuals(visuals);
}

/// An icon glyph (one of [`egui_phosphor::bold`]'s constants) as widget text at
/// [`ICON_SIZE`]. Callers add color like any other [`egui::RichText`].
pub fn icon(glyph: &str) -> egui::RichText {
    egui::RichText::new(glyph).size(ICON_SIZE)
}

/// The filled counterpart of [`icon`], for the "on" half of a pair (a saved
/// bookmark against an unsaved one). Takes an [`egui_phosphor::fill`] constant.
pub fn icon_fill(glyph: &str) -> egui::RichText {
    egui::RichText::new(glyph)
        .size(ICON_SIZE)
        .family(egui::FontFamily::Name(FILL_FAMILY.into()))
}

/// Join control hints (`"A open"`, ...) into one line, a small dot between each.
pub fn hint_line(hints: &[&str]) -> String {
    hints.join(&format!("  {}  ", egui_phosphor::bold::DOT))
}

/// Side of the square close button (logical px).
pub const CLOSE_SIZE: f32 = 28.0;

/// A mouse-only close button drawn at `rect`; a gamepad closes the overlay with
/// B instead. `id` must be unique per call site — two overlays can be on screen
/// at once.
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
