//! Shared UI palette, icon font, and the egui visual theme. [`apply`] installs
//! them on the egui context at startup, so every `selectable` widget, text
//! selection and link picks the accent up without per-widget styling. [`set`]
//! switches the palette; [`apply_visuals`] pushes it onto a live context.

use crate::config::UiTheme;
use egui_phosphor::Variant;
use egui_sdl2::egui;
use std::sync::{PoisonError, RwLock};

/// The chrome's colors under one [`UiTheme`].
pub struct Palette {
    /// The brand accent (teal-green) — selected/active emphasis across the UI.
    pub accent: egui::Color32,
    /// Dark chrome panel fill, shared by the menu / settings / prompt overlays.
    pub panel_fill: egui::Color32,
    /// Secondary / label text: hints, dates, statuses, mouse-only actions.
    pub dim: egui::Color32,
    /// Backdrop under something that blocks: what it covers reads as inactive.
    pub scrim: egui::Color32,
    /// A refused or destructive action: the success teal cannot read as either.
    pub warn: egui::Color32,
    // The start-page palette, shared by the dial editor (its tiles must match).
    pub bg: egui::Color32,
    pub surface: egui::Color32,
    pub border: egui::Color32,
    pub ink: egui::Color32,
    pub muted: egui::Color32,
    /// The icon's far wave: the accent darkened, for edges that must read on light
    /// pages without the weight of black.
    pub deep_teal: egui::Color32,
    /// The wordmark gradient's warm (coral) end, paired with `accent`.
    pub surf_warm: egui::Color32,
    /// Hairline ring around a floating card, and the memory overlay's border.
    pub hairline: egui::Color32,
    /// Text on the overlays: rows, labels, buttons.
    pub text: egui::Color32,
    /// A row or action that cannot be taken now.
    pub disabled: egui::Color32,
    /// egui's own window and panel fill, under the widgets it draws.
    pub window: egui::Color32,
    /// egui's text field background.
    pub field: egui::Color32,
}

/// The brand's dark slate, and the default.
const NIGHT: Palette = Palette {
    accent: egui::Color32::from_rgb(0x3f, 0xb8, 0xa0),
    panel_fill: egui::Color32::from_rgb(0x18, 0x18, 0x1c),
    dim: egui::Color32::from_gray(0x99),
    scrim: egui::Color32::from_black_alpha(140),
    warn: egui::Color32::from_rgb(0xe8, 0x73, 0x5a),
    bg: egui::Color32::from_rgb(0x16, 0x17, 0x1a),
    surface: egui::Color32::from_rgb(0x1e, 0x20, 0x24),
    border: egui::Color32::from_rgb(0x2a, 0x2d, 0x33),
    ink: egui::Color32::from_rgb(0xec, 0xec, 0xea),
    muted: egui::Color32::from_rgb(0x8a, 0x8f, 0x98),
    deep_teal: egui::Color32::from_rgb(0x26, 0x57, 0x50),
    surf_warm: egui::Color32::from_rgb(0xff, 0x8c, 0x69),
    hairline: egui::Color32::from_gray(0x55),
    text: egui::Color32::WHITE,
    disabled: egui::Color32::from_gray(0x66),
    window: egui::Color32::from_gray(0x1b),
    field: egui::Color32::from_gray(0x0a),
};

/// Black under everything, for OLED screens, whose black pixels stay unlit.
const OLED: Palette = Palette {
    panel_fill: egui::Color32::BLACK,
    bg: egui::Color32::BLACK,
    surface: egui::Color32::from_rgb(0x10, 0x11, 0x13),
    border: egui::Color32::from_rgb(0x22, 0x24, 0x28),
    window: egui::Color32::BLACK,
    field: egui::Color32::BLACK,
    ..NIGHT
};

/// The theme in force; [`palette`] reads it on every draw.
static CURRENT: RwLock<UiTheme> = RwLock::new(UiTheme::Night);

/// Switch the palette. A live context also needs [`apply_visuals`].
pub fn set(theme: UiTheme) {
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = theme;
}

/// The colors of the theme in force.
pub fn palette() -> &'static Palette {
    match *CURRENT.read().unwrap_or_else(PoisonError::into_inner) {
        UiTheme::Night => &NIGHT,
        UiTheme::Oled => &OLED,
    }
}

/// The centered floating card (modal prompt, game menu): one frame, so the two
/// read as the same surface.
pub fn card_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(palette().panel_fill)
        .stroke(egui::Stroke::new(1.0, palette().hairline))
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

/// The UI's text face. It leads the proportional family, so its metrics set
/// the rows; it maps U+ECE0, so Phosphor's `DOT_OUTLINE` would draw as its
/// glyph instead.
const UI_FONT: &str = "IBMPlexSans-Regular";

/// Icon size for buttons and rows. Phosphor draws its artwork inset in the em
/// box, so an icon needs a few points over the body text to match it optically.
pub const ICON_SIZE: f32 = 15.0;

/// `svg` with each of Night's brand colors swapped for the palette in force's.
/// The bundled brand SVGs are drawn in Night's colors, as lowercase `#rrggbb`.
pub fn recolor_svg(svg: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let p = palette();
    if std::ptr::eq(p, &NIGHT) {
        return std::borrow::Cow::Borrowed(svg);
    }
    let swaps = [
        (NIGHT.bg, p.bg),
        (NIGHT.ink, p.ink),
        (NIGHT.accent, p.accent),
        (NIGHT.surf_warm, p.surf_warm),
        (NIGHT.deep_teal, p.deep_teal),
    ]
    .map(|(from, to)| (hex(from), hex(to)));
    // One pass, so a color swapped in is never swapped again.
    let mut out = Vec::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.iter().position(|&b| b == b'#') {
        out.extend_from_slice(&rest[..at]);
        rest = &rest[at..];
        match swaps
            .iter()
            .find(|(from, _)| rest.starts_with(from.as_bytes()))
        {
            Some((from, to)) => {
                out.extend_from_slice(to.as_bytes());
                rest = &rest[from.len()..];
            }
            None => {
                out.push(b'#');
                rest = &rest[1..];
            }
        }
    }
    out.extend_from_slice(rest);
    std::borrow::Cow::Owned(out)
}

/// `color` as SVG writes it: lowercase `#rrggbb`.
fn hex(color: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// Install the icon font and the palette's visuals (see [`apply_visuals`]).
pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        UI_FONT.to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../resources/fonts/IBMPlexSans-Regular.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, UI_FONT.to_owned());
    // `add_to_fonts` puts Phosphor *second*: the UI face keeps the family's
    // metrics, and Phosphor still wins the private-use range the emoji faces map.
    egui_phosphor::add_to_fonts(&mut fonts, ICON_VARIANT);
    fonts
        .font_data
        .insert(FILL_FAMILY.to_owned(), Variant::Fill.font_data().into());
    // The UI face leads this family too, so a filled icon lays out identically
    // to the outlined one it swaps with.
    fonts.families.insert(
        egui::FontFamily::Name(FILL_FAMILY.into()),
        vec![UI_FONT.to_owned(), FILL_FAMILY.to_owned()],
    );
    ctx.set_fonts(fonts);
    apply_visuals(ctx);
}

/// Put the palette in force onto egui's dark theme: its fills, a translucent
/// accent fill behind selected widgets, ringed by the solid accent, plus
/// accent-colored links and caret.
pub fn apply_visuals(ctx: &egui::Context) {
    let p = palette();
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = p.window;
    visuals.panel_fill = p.window;
    visuals.widgets.noninteractive.bg_fill = p.window;
    visuals.widgets.noninteractive.weak_bg_fill = p.window;
    visuals.extreme_bg_color = p.field;
    // A low-alpha wash tints the row without swamping the text over it.
    visuals.selection.bg_fill = p.accent.linear_multiply(0.30);
    visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);
    visuals.hyperlink_color = p.accent;
    visuals.text_cursor.stroke.color = p.accent;
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

/// Component-wise sRGB lerp. Fine for a subtle brand tint (no need for linear space).
pub fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
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
        palette().accent
    } else {
        egui::Color32::from_gray(0x44)
    };
    let ink = if hot {
        palette().accent
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
