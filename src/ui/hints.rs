//! Rendering of the link-hint overlay (state lives in [`crate::overlay::hints`]):
//! a thin accent frame on every clickable element (the selected one filled and
//! outlined brighter for the spatial stick hop), plus a Vimium-style combo-code
//! badge per element — the gamepad buttons you press to jump straight to it.
//!
//! Button glyphs aren't in egui's bundled fonts (see the `retsurf-egui-glyph-
//! coverage` memory), so the symbols are painter shapes: lettered pills for the
//! face/shoulder buttons (X / Y / L1 / R1, plain ASCII) and filled triangles for
//! the four D-pad directions. While a combo is being typed, the already-pressed
//! leading cells dim and the hints whose codes no longer match fade out, leaving
//! only the live targets — the spatial selection frame stays so A still works.

use super::theme::ACCENT;
use crate::overlay::hints::{Hints, Sym};
use egui_sdl2::egui;

/// Height of one combo-symbol cell (logical px); the badge is a row of these.
const CELL_H: f32 = 13.0;
/// Width of a single-glyph cell (a letter or a D-pad triangle).
const CELL_W: f32 = 12.0;
/// Width of a two-character cell (`L1` / `R1`).
const CELL_W_WIDE: f32 = 18.0;
/// Outer-corner rounding of a code's pill row (cells abut with no gap).
const CELL_RADIUS: u8 = 2;
/// Dark ink for glyphs drawn over the bright accent pill.
const PILL_INK: egui::Color32 = egui::Color32::from_rgb(0x10, 0x16, 0x14);
/// Muted pill + ink for an already-pressed (leading) cell.
const DONE_PILL: egui::Color32 = egui::Color32::from_gray(0x3a);
const DONE_INK: egui::Color32 = egui::Color32::from_gray(0xc0);

pub(super) fn add_hints(ctx: &egui::Context, hints: &Hints, webview: egui::Rect, badges: bool) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("hints"),
    ));
    let frame = egui::Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 110);
    let selected_fill =
        egui::Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 45);
    let typed = hints.typed();

    for (i, hint) in hints.hints().iter().enumerate() {
        // Rects are viewport-relative; shift by the web-view origin.
        let rect = egui::Rect::from_min_size(
            egui::pos2(hint.x + webview.left(), hint.y + webview.top()),
            egui::vec2(hint.w, hint.h),
        );
        // Hints whose code no longer prefixes the typed buffer fade out; the rest
        // (all of them when nothing is typed) stay at full strength.
        let matched = hints.matches_prefix(i);
        let fade = if matched { 1.0 } else { 0.16 };

        if i == hints.selected() {
            painter.rect(
                rect.expand(2.0),
                3.0,
                selected_fill.gamma_multiply(fade),
                egui::Stroke::new(2.0, ACCENT.gamma_multiply(fade)),
                egui::StrokeKind::Outside,
            );
        } else {
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, frame.gamma_multiply(fade)),
                egui::StrokeKind::Outside,
            );
        }

        // Only the live targets carry a readable badge; faded hints drop it.
        // With combos disabled, no badges at all — just the spatial frames.
        if badges && matched {
            draw_badge(&painter, rect, hints.code(i), typed.len());
        }
    }
}

/// Draw the combo code as a row of symbol cells at the rect's top-left corner.
/// `done` is how many leading symbols are already typed (drawn muted).
fn draw_badge(painter: &egui::Painter, rect: egui::Rect, code: &[Sym], done: usize) {
    let mut x = rect.left();
    let top = rect.top();
    let last = code.len().saturating_sub(1);
    for (j, &sym) in code.iter().enumerate() {
        let w = cell_width(sym);
        let cell = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, CELL_H));
        let (pill, ink) = if j < done {
            (DONE_PILL, DONE_INK)
        } else {
            (ACCENT, PILL_INK)
        };
        // Cells abut with no gap; round only the row's outer corners so a code
        // reads as one pill.
        painter.rect_filled(cell, group_corners(j, last), pill);
        draw_sym(painter, cell, sym, ink);
        x += w;
    }
}

/// Corner rounding for cell `j` of a `last`-indexed code row: left corners on the
/// first cell, right corners on the last, square between — one merged pill.
fn group_corners(j: usize, last: usize) -> egui::CornerRadius {
    let left = if j == 0 { CELL_RADIUS } else { 0 };
    let right = if j == last { CELL_RADIUS } else { 0 };
    egui::CornerRadius {
        nw: left,
        sw: left,
        ne: right,
        se: right,
    }
}

fn cell_width(sym: Sym) -> f32 {
    match sym {
        Sym::L1 | Sym::R1 => CELL_W_WIDE,
        _ => CELL_W,
    }
}

/// Render one symbol centered in its pill: a letter for the buttons, a filled
/// triangle for the D-pad directions.
fn draw_sym(painter: &egui::Painter, cell: egui::Rect, sym: Sym, ink: egui::Color32) {
    let label = match sym {
        Sym::X => "X",
        Sym::Y => "Y",
        Sym::L1 => "L1",
        Sym::R1 => "R1",
        _ => {
            triangle(painter, cell, sym, ink);
            return;
        }
    };
    let size = if label.len() == 2 { 9.0 } else { 10.0 };
    painter.text(
        cell.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(size),
        ink,
    );
}

/// A small filled triangle pointing in the D-pad direction, centered in `cell`.
fn triangle(painter: &egui::Painter, cell: egui::Rect, sym: Sym, ink: egui::Color32) {
    let c = cell.center();
    let r = 3.0; // half-extent from the center
    let pts = match sym {
        Sym::Up => [
            egui::pos2(c.x, c.y - r),
            egui::pos2(c.x - r, c.y + r),
            egui::pos2(c.x + r, c.y + r),
        ],
        Sym::Down => [
            egui::pos2(c.x, c.y + r),
            egui::pos2(c.x - r, c.y - r),
            egui::pos2(c.x + r, c.y - r),
        ],
        Sym::Left => [
            egui::pos2(c.x - r, c.y),
            egui::pos2(c.x + r, c.y - r),
            egui::pos2(c.x + r, c.y + r),
        ],
        // Right (the only remaining direction).
        _ => [
            egui::pos2(c.x + r, c.y),
            egui::pos2(c.x - r, c.y - r),
            egui::pos2(c.x - r, c.y + r),
        ],
    };
    painter.add(egui::Shape::convex_polygon(
        pts.to_vec(),
        ink,
        egui::Stroke::NONE,
    ));
}
