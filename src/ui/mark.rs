//! Icons painted rather than set in a font, because each shows state: the open
//! tabs counted inside a square, and the downloads as a ring around their glyph.
//! A mark is sized by the glyphs it stands beside, so the toolbar and a smaller
//! bar show the same shapes.

use super::download_chip::Chip;
use super::theme;
use super::toolbar::ICON;
use egui_phosphor::bold;
use egui_sdl2::egui;
use std::f32::consts::TAU;

/// The tab square beside a [`theme::ICON_SIZE`] glyph: its half side, outline,
/// corner, and the count's size for one digit and for two.
const TABS_HALF: f32 = 6.5;
const TABS_STROKE: f32 = 1.5;
const TABS_CORNER: f32 = 2.0;
const TABS_FONT: f32 = 10.0;
const TABS_FONT_TWO: f32 = 8.0;
/// The highest count the square holds.
const TABS_CAP: usize = 99;

/// The downloads ring's radius and stroke beside an [`ICON`] glyph.
pub(super) const RING_R: f32 = 13.0;
const RING_W: f32 = 2.0;

/// The ring's empty track: the muted ink, faded.
const RING_TRACK_ALPHA: f32 = 0.35;

/// Points on a full turn of the ring's arc.
const RING_POINTS: f32 = 48.0;

/// The arc standing in for an unknown size, and its turn per step.
const UNKNOWN_SWEEP: f32 = TAU / 4.0;
const UNKNOWN_STEP: f32 = TAU / 8.0;

/// A painted icon and the state it shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mark {
    /// Open tabs, counted inside a square.
    Tabs(usize),
    /// Downloads in flight or just ended; [`Chip::Hidden`] paints nothing.
    Downloads(Chip),
}

impl Mark {
    /// The box the mark fills beside glyphs `glyph` px tall.
    pub fn size(self, glyph: f32) -> egui::Vec2 {
        match self {
            Mark::Tabs(_) => egui::Vec2::splat(2.0 * TABS_HALF * glyph / theme::ICON_SIZE),
            Mark::Downloads(_) => egui::Vec2::splat((2.0 * RING_R + RING_W) * glyph / ICON),
        }
    }

    /// Paint the mark centred in `rect`, beside glyphs `glyph` px tall. `ink`
    /// colours a mark whose state has no colour of its own.
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect, glyph: f32, ink: egui::Color32) {
        match self {
            Mark::Tabs(count) => {
                paint_tabs(painter, rect.center(), glyph / theme::ICON_SIZE, count, ink)
            }
            Mark::Downloads(chip) => paint_ring(painter, rect.center(), glyph / ICON, chip),
        }
    }
}

/// Paint a rounded square with `count` inside, at `scale`.
fn paint_tabs(
    painter: &egui::Painter,
    center: egui::Pos2,
    scale: f32,
    count: usize,
    color: egui::Color32,
) {
    // Snap the center onto a half-pixel so the 1px stroke lands on whole pixels.
    let c = center.floor() + egui::vec2(0.5, 0.5);
    let square = egui::Rect::from_center_size(c, egui::Vec2::splat(2.0 * TABS_HALF * scale));
    painter.rect_stroke(
        square,
        TABS_CORNER,
        egui::Stroke::new(TABS_STROKE, color),
        egui::StrokeKind::Inside,
    );
    let size = match count > 9 {
        true => TABS_FONT_TWO,
        false => TABS_FONT,
    };
    let galley = painter.layout_no_wrap(
        count.min(TABS_CAP).to_string(),
        egui::FontId::proportional(scale * size),
        color,
    );
    // Centered on the digits' ink: the font's line box leaves descender room below them.
    painter.galley(c - galley.mesh_bounds.center().to_vec2(), galley, color);
}

/// Paint the downloads as a ring around their glyph at `scale`, filled by the
/// share done or stepping an arc while a size is unknown, then a check or a
/// cross once they ended.
fn paint_ring(painter: &egui::Painter, c: egui::Pos2, scale: f32, chip: Chip) {
    let palette = theme::palette();
    let (glyph, color, arc) = match chip {
        Chip::Hidden => return,
        Chip::Running { share, step, .. } => {
            let arc = match share {
                Some(share) => (0.0, TAU * f32::from(share) / f32::from(Chip::SHARE_FULL)),
                None => (step as f32 * UNKNOWN_STEP, UNKNOWN_SWEEP),
            };
            (bold::DOWNLOAD_SIMPLE, palette.accent, arc)
        }
        Chip::Ended { failed: false } => (bold::CHECK, palette.accent, (0.0, TAU)),
        Chip::Ended { failed: true } => (bold::X, palette.warn, (0.0, TAU)),
    };
    let (radius, width) = (RING_R * scale, RING_W * scale);
    let track = palette.muted.gamma_multiply(RING_TRACK_ALPHA);
    painter.circle_stroke(c, radius, egui::Stroke::new(width, track));
    let (start, sweep) = arc;
    if sweep > 0.0 {
        // Clockwise from twelve o'clock.
        let n = (sweep / TAU * RING_POINTS).ceil().max(1.0) as usize;
        let points = (0..=n)
            .map(|i| {
                let a = start + sweep * i as f32 / n as f32;
                c + radius * egui::vec2(a.sin(), -a.cos())
            })
            .collect();
        painter.add(egui::Shape::line(points, egui::Stroke::new(width, color)));
    }
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(theme::ICON_SIZE * scale),
        color,
    );
}
