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
    /// egui's buttons and switch tracks, at rest and hovered.
    pub button: egui::Color32,
    pub button_hover: egui::Color32,
    /// A key or chip raised off a panel: the on-screen keyboard's keys.
    pub key: egui::Color32,
    /// A button badge's fill: dimmer than the words beside it, its label in `bg`.
    pub cap: egui::Color32,
    /// Ink over an accent fill: a link hint's code.
    pub on_accent: egui::Color32,
    /// An already-pressed hint cell, and its ink.
    pub spent: egui::Color32,
    pub spent_ink: egui::Color32,
    /// A mouse-only control at rest: its ring, and its mark.
    pub control_line: egui::Color32,
    pub control_ink: egui::Color32,
    /// The brand sun gradient's top stop, warmer than `surf_warm`.
    pub sun_top: egui::Color32,
    /// The brand's middle wave; the far one is `deep_teal`.
    pub wave_mid: egui::Color32,
    /// The painted sea under the brand's horizon wave.
    pub sea: egui::Color32,
    /// How far the brand sun sits below its Night height, as a share of its
    /// diameter; below zero it has risen.
    pub sun_drop: f32,
    /// Whether egui's light visuals underlie the theme.
    pub light: bool,
    /// The wheel keyboard's colors.
    pub wheel: Wheel,
}

/// The wheel keyboard's colors; its face buttons keep the pad's own.
pub struct Wheel {
    /// The ring the groups sit on, and the hub at its center.
    pub rim: egui::Color32,
    pub hub: egui::Color32,
    /// A group of four characters, and the one the stick aims at.
    pub group: egui::Color32,
    pub aimed: egui::Color32,
    /// A face button outside the aimed group: muted, so only the aimed one reads.
    pub face_idle: egui::Color32,
    /// A tab's fill, and its action, dimmer than its button so the two read as a pair.
    pub tab: egui::Color32,
    pub tab_action: egui::Color32,
    /// Hub marks while a group is aimed.
    pub hub_dim: egui::Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

const fn gray(l: u8) -> egui::Color32 {
    egui::Color32::from_gray(l)
}

/// The brand's dark slate, whose colors the bundled brand SVGs are drawn in.
const NIGHT: Palette = Palette {
    accent: rgb(0x3f, 0xb8, 0xa0),
    panel_fill: rgb(0x18, 0x18, 0x1c),
    dim: gray(0x99),
    scrim: egui::Color32::from_black_alpha(140),
    warn: rgb(0xe8, 0x73, 0x5a),
    bg: rgb(0x16, 0x17, 0x1a),
    surface: rgb(0x1e, 0x20, 0x24),
    border: rgb(0x2a, 0x2d, 0x33),
    ink: rgb(0xec, 0xec, 0xea),
    muted: rgb(0x8a, 0x8f, 0x98),
    deep_teal: rgb(0x26, 0x57, 0x50),
    surf_warm: rgb(0xff, 0x8c, 0x69),
    hairline: gray(0x55),
    text: egui::Color32::WHITE,
    disabled: gray(0x66),
    window: gray(0x1b),
    field: gray(0x0a),
    button: gray(0x3c),
    button_hover: gray(0x46),
    key: rgb(0x3a, 0x3a, 0x40),
    cap: gray(0x5c),
    on_accent: rgb(0x10, 0x16, 0x14),
    spent: gray(0x3a),
    spent_ink: gray(0xc0),
    control_line: gray(0x44),
    control_ink: gray(0xe0),
    sun_top: rgb(0xff, 0xb8, 0x70),
    wave_mid: rgb(0x33, 0x88, 0x78),
    sea: rgb(0x1a, 0x2a, 0x2a),
    sun_drop: 0.0,
    light: false,
    wheel: Wheel {
        rim: egui::Color32::from_rgba_premultiplied(0x1c, 0x1c, 0x1e, 0xe0),
        hub: egui::Color32::from_rgba_premultiplied(0x34, 0x34, 0x36, 0xa0),
        group: gray(0x48),
        aimed: gray(0x62),
        face_idle: gray(0x2c),
        tab: gray(0x36),
        tab_action: gray(0xa8),
        hub_dim: gray(0x80),
    },
};

/// Black under everything, for OLED screens, whose black pixels stay unlit; the
/// sun has nearly set.
const OLED: Palette = Palette {
    panel_fill: egui::Color32::BLACK,
    bg: egui::Color32::BLACK,
    surface: rgb(0x10, 0x11, 0x13),
    border: rgb(0x22, 0x24, 0x28),
    window: egui::Color32::BLACK,
    field: egui::Color32::BLACK,
    sea: rgb(0x08, 0x12, 0x12),
    sun_drop: 0.25,
    ..NIGHT
};

/// Night warmed toward plum, the sun lower.
const DUSK: Palette = Palette {
    panel_fill: rgb(0x1e, 0x18, 0x21),
    dim: rgb(0xa8, 0x9a, 0xa0),
    bg: rgb(0x1c, 0x16, 0x1f),
    surface: rgb(0x26, 0x1e, 0x29),
    border: rgb(0x36, 0x2c, 0x3a),
    ink: rgb(0xf0, 0xe6, 0xe2),
    muted: rgb(0x9e, 0x8f, 0x98),
    hairline: rgb(0x5a, 0x4c, 0x58),
    text: rgb(0xfa, 0xf2, 0xee),
    disabled: rgb(0x6e, 0x60, 0x6a),
    window: rgb(0x22, 0x1b, 0x25),
    field: rgb(0x12, 0x0e, 0x14),
    button: rgb(0x44, 0x38, 0x48),
    button_hover: rgb(0x4e, 0x42, 0x52),
    key: rgb(0x3e, 0x33, 0x42),
    cap: rgb(0x62, 0x55, 0x63),
    spent: rgb(0x3e, 0x33, 0x42),
    control_line: rgb(0x4a, 0x3d, 0x4c),
    sea: rgb(0x2a, 0x1e, 0x2c),
    sun_drop: 0.12,
    wheel: Wheel {
        rim: egui::Color32::from_rgba_premultiplied(0x1c, 0x15, 0x1e, 0xe0),
        hub: egui::Color32::from_rgba_premultiplied(0x23, 0x1d, 0x24, 0xa0),
        group: rgb(0x4c, 0x40, 0x4e),
        aimed: rgb(0x66, 0x58, 0x68),
        face_idle: rgb(0x30, 0x26, 0x32),
        tab: rgb(0x3a, 0x2f, 0x3c),
        tab_action: rgb(0xae, 0xa0, 0xa8),
        hub_dim: rgb(0x86, 0x78, 0x82),
    },
    ..NIGHT
};

/// Daylight on the beach: dark ink on sand, the accent deepened to read on it,
/// the sun high.
const SAND: Palette = Palette {
    accent: rgb(0x19, 0x66, 0x57),
    panel_fill: rgb(0xef, 0xe7, 0xd9),
    dim: rgb(0x5c, 0x5f, 0x66),
    scrim: egui::Color32::from_black_alpha(90),
    warn: rgb(0xb4, 0x43, 0x2a),
    bg: rgb(0xe6, 0xdc, 0xcb),
    surface: rgb(0xf1, 0xea, 0xdf),
    border: rgb(0xcf, 0xc2, 0xab),
    ink: rgb(0x16, 0x17, 0x1a),
    muted: rgb(0x5c, 0x5f, 0x66),
    deep_teal: rgb(0x10, 0x46, 0x3d),
    surf_warm: rgb(0xd9, 0x66, 0x45),
    hairline: rgb(0xbd, 0xb0, 0x98),
    text: rgb(0x16, 0x17, 0x1a),
    disabled: rgb(0xa3, 0x99, 0x88),
    window: rgb(0xef, 0xe7, 0xd9),
    field: rgb(0xfb, 0xf7, 0xf0),
    button: rgb(0xdd, 0xd2, 0xbe),
    button_hover: rgb(0xd3, 0xc6, 0xae),
    key: rgb(0xdd, 0xd2, 0xbe),
    cap: rgb(0x8a, 0x80, 0x70),
    on_accent: rgb(0xf4, 0xef, 0xe6),
    spent: rgb(0xd6, 0xcb, 0xb8),
    spent_ink: rgb(0x5c, 0x5f, 0x66),
    control_line: rgb(0xbd, 0xb0, 0x98),
    control_ink: rgb(0x2a, 0x2b, 0x2e),
    sun_top: rgb(0xff, 0xb8, 0x70),
    wave_mid: rgb(0x2f, 0x7d, 0x6f),
    sea: rgb(0xd5, 0xe2, 0xda),
    sun_drop: -0.45,
    light: true,
    wheel: Wheel {
        rim: egui::Color32::from_rgba_premultiplied(0xd2, 0xcb, 0xbf, 0xe0),
        hub: egui::Color32::from_rgba_premultiplied(0x87, 0x80, 0x74, 0xa0),
        group: rgb(0xd8, 0xcd, 0xb9),
        aimed: rgb(0xc4, 0xb7, 0x9f),
        face_idle: rgb(0xe8, 0xe0, 0xd2),
        tab: rgb(0xdd, 0xd2, 0xbf),
        tab_action: rgb(0x5c, 0x5f, 0x66),
        hub_dim: rgb(0x8a, 0x80, 0x70),
    },
};

/// The original Game Boy's four greens, dark on light like its screen.
const DMG: Palette = Palette {
    accent: rgb(0x30, 0x62, 0x30),
    panel_fill: rgb(0x9b, 0xbc, 0x0f),
    dim: rgb(0x30, 0x62, 0x30),
    scrim: egui::Color32::from_black_alpha(80),
    warn: rgb(0x0f, 0x38, 0x0f),
    bg: rgb(0x9b, 0xbc, 0x0f),
    surface: rgb(0x8b, 0xac, 0x0f),
    border: rgb(0x30, 0x62, 0x30),
    ink: rgb(0x0f, 0x38, 0x0f),
    muted: rgb(0x30, 0x62, 0x30),
    deep_teal: rgb(0x0f, 0x38, 0x0f),
    surf_warm: rgb(0x0f, 0x38, 0x0f),
    hairline: rgb(0x30, 0x62, 0x30),
    text: rgb(0x0f, 0x38, 0x0f),
    disabled: rgb(0x5a, 0x7a, 0x1a),
    window: rgb(0x9b, 0xbc, 0x0f),
    field: rgb(0x8b, 0xac, 0x0f),
    button: rgb(0x8b, 0xac, 0x0f),
    button_hover: rgb(0x7f, 0x9f, 0x10),
    key: rgb(0x8b, 0xac, 0x0f),
    cap: rgb(0x30, 0x62, 0x30),
    on_accent: rgb(0x9b, 0xbc, 0x0f),
    spent: rgb(0x8b, 0xac, 0x0f),
    spent_ink: rgb(0x30, 0x62, 0x30),
    control_line: rgb(0x30, 0x62, 0x30),
    control_ink: rgb(0x0f, 0x38, 0x0f),
    sun_top: rgb(0x30, 0x62, 0x30),
    wave_mid: rgb(0x30, 0x62, 0x30),
    sea: rgb(0x8b, 0xac, 0x0f),
    sun_drop: 0.0,
    light: true,
    wheel: Wheel {
        rim: egui::Color32::from_rgba_premultiplied(0x2a, 0x56, 0x2a, 0xe0),
        hub: egui::Color32::from_rgba_premultiplied(0x09, 0x23, 0x09, 0xa0),
        group: rgb(0x8b, 0xac, 0x0f),
        aimed: rgb(0x9b, 0xbc, 0x0f),
        face_idle: rgb(0x9b, 0xbc, 0x0f),
        tab: rgb(0x8b, 0xac, 0x0f),
        tab_action: rgb(0x30, 0x62, 0x30),
        hub_dim: rgb(0x8b, 0xac, 0x0f),
    },
};

/// The theme in force; [`palette`] reads it on every draw.
static CURRENT: RwLock<UiTheme> = RwLock::new(UiTheme::Dusk);

/// Switch the palette. A live context also needs [`apply_visuals`].
pub fn set(theme: UiTheme) {
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = theme;
}

/// The colors of the theme in force.
pub fn palette() -> &'static Palette {
    match *CURRENT.read().unwrap_or_else(PoisonError::into_inner) {
        UiTheme::Night => &NIGHT,
        UiTheme::Oled => &OLED,
        UiTheme::Dusk => &DUSK,
        UiTheme::Sand => &SAND,
        UiTheme::Dmg => &DMG,
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

/// `svg` with each of Night's brand colors swapped for the palette in force's,
/// and its sun, of `sun_diameter` in the SVG's units, moved by the palette's drop.
/// The bundled brand SVGs are drawn in Night's colors, as lowercase `#rrggbb`; the
/// sun is the group with `id="sun"` and a `translate(x y)` first in its transform.
pub fn recolor_svg(svg: &[u8], sun_diameter: Option<f32>) -> std::borrow::Cow<'_, [u8]> {
    let p = palette();
    if std::ptr::eq(p, &NIGHT) {
        return std::borrow::Cow::Borrowed(svg);
    }
    let svg = match sun_diameter {
        Some(d) if p.sun_drop != 0.0 => std::borrow::Cow::Owned(drop_sun(svg, p.sun_drop * d)),
        _ => std::borrow::Cow::Borrowed(svg),
    };
    let swaps = [
        (NIGHT.bg, p.bg),
        (NIGHT.ink, p.ink),
        (NIGHT.accent, p.accent),
        (NIGHT.surf_warm, p.surf_warm),
        (NIGHT.deep_teal, p.deep_teal),
        (NIGHT.sun_top, p.sun_top),
        (NIGHT.wave_mid, p.wave_mid),
    ]
    .map(|(from, to)| (hex(from), hex(to)));
    // One pass, so a color swapped in is never swapped again.
    let mut out = Vec::with_capacity(svg.len());
    let mut rest: &[u8] = &svg;
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

/// `svg` with the sun group's translation moved down by `dy`; unchanged without one.
fn drop_sun(svg: &[u8], dy: f32) -> Vec<u8> {
    const MARK: &str = r#"id="sun" transform="translate("#;
    let text = String::from_utf8_lossy(svg);
    let Some(at) = text.find(MARK).map(|i| i + MARK.len()) else {
        return svg.to_vec();
    };
    let Some(len) = text[at..].find(')') else {
        return svg.to_vec();
    };
    let mut xy = text[at..at + len].split_whitespace().map(str::parse::<f32>);
    let (Some(Ok(x)), Some(Ok(y))) = (xy.next(), xy.next()) else {
        return svg.to_vec();
    };
    format!("{}{x} {}{}", &text[..at], y + dy, &text[at + len..]).into_bytes()
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

/// Put the palette in force onto egui's dark or light theme: its fills, a translucent
/// accent fill behind selected widgets, ringed by the solid accent, plus
/// accent-colored links and caret.
pub fn apply_visuals(ctx: &egui::Context) {
    let p = palette();
    let mut visuals = match p.light {
        true => egui::Visuals::light(),
        false => egui::Visuals::dark(),
    };
    visuals.window_fill = p.window;
    visuals.panel_fill = p.window;
    visuals.widgets.noninteractive.bg_fill = p.window;
    visuals.widgets.noninteractive.weak_bg_fill = p.window;
    visuals.extreme_bg_color = p.field;
    for (widget, fill) in [
        (&mut visuals.widgets.inactive, p.button),
        (&mut visuals.widgets.hovered, p.button_hover),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
    }
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
        palette().control_line
    };
    let ink = if hot {
        palette().accent
    } else {
        palette().control_ink
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
