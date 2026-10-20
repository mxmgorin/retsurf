//! Buttons and keys named on screen, each drawn as a solid badge so it reads
//! apart from the words around it: a circle for a one-glyph button (a face
//! button, a D-pad direction), a pill for a longer name (L1, Start, Esc). The
//! hint rows built from them share one measure, so a row is laid out before it
//! is drawn.

use super::theme::{BG, DIM};
use egui_sdl2::egui;

/// Type size of a hint row.
pub(super) const HINT_FONT: f32 = 13.0;
/// Space between one hint and the next.
const HINT_GAP: f32 = 16.0;
/// Space between a badge and what it is next to.
const CAP_GAP: f32 = 5.0;
/// A badge's height, as a share of its label's type size.
const CAP_HEIGHT: f32 = 1.15;
/// A pill's inner margin across, as a share of its label's type size.
const PILL_MARGIN: f32 = 0.35;
/// A badge label's size, as a share of the size asked for.
const CAP_FONT: f32 = 0.78;
/// A badge's fill: dimmer than the words beside it, the label in the chrome's dark.
const CAP_FILL: egui::Color32 = egui::Color32::from_gray(0x5c);

/// The shoulders that step a section or a filter.
pub(super) const SHOULDERS: [&str; 2] = ["L1", "R1"];

/// One button or key as a badge, its label at `size`.
pub(super) fn key_cap(ui: &mut egui::Ui, key: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(cap_size(ui, key, size), egui::Sense::hover());
    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    match is_round(key) {
        true => painter.circle_filled(rect.center(), radius, CAP_FILL),
        false => painter.rect_filled(rect, radius, CAP_FILL),
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        key,
        egui::FontId::proportional(size * CAP_FONT),
        BG,
    );
}

/// A one-glyph label gets a circle.
fn is_round(key: &str) -> bool {
    key.chars().count() == 1
}

/// The size [`key_cap`] takes.
pub(super) fn cap_size(ui: &egui::Ui, key: &str, size: f32) -> egui::Vec2 {
    let height = size * CAP_HEIGHT;
    let width = match is_round(key) {
        true => height,
        false => (text_width(ui, key, size * CAP_FONT) + 2.0 * size * PILL_MARGIN).max(height),
    };
    egui::vec2(width, height)
}

/// The D-pad's axes as one badge each: up with down, then left with right.
pub(super) fn dpad_caps() -> (String, String) {
    use egui_phosphor::bold::{CARET_DOWN, CARET_LEFT, CARET_RIGHT, CARET_UP};
    (
        format!("{CARET_UP}{CARET_DOWN}"),
        format!("{CARET_LEFT}{CARET_RIGHT}"),
    )
}

/// A hint: the badges of what does it, then what it does. No badges is a note.
pub(super) type Hint<'a> = (&'a [&'a str], &'a str);

/// A row of hints, wrapping where the width runs out.
pub(super) fn hint_row(ui: &mut egui::Ui, hints: &[Hint]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(CAP_GAP, CAP_GAP);
        for (i, hint) in hints.iter().enumerate() {
            if i > 0 {
                ui.add_space(HINT_GAP - CAP_GAP);
            }
            add_hint(ui, hint);
        }
    });
}

/// `hints` in lines centred across `width`, broken between hints and never
/// inside one; returns the height they took.
pub(super) fn centered_hints(ui: &mut egui::Ui, hints: &[Hint], width: f32) -> f32 {
    let widths: Vec<f32> = hints.iter().map(|hint| hint_width(ui, hint)).collect();
    let mut lines: Vec<std::ops::Range<usize>> = Vec::new();
    let (mut start, mut used) = (0, 0.0);
    for (i, &w) in widths.iter().enumerate() {
        if i > start && used + HINT_GAP + w > width {
            lines.push(start..i);
            (start, used) = (i, w);
        } else {
            used += if i == start { w } else { HINT_GAP + w };
        }
    }
    if start < hints.len() {
        lines.push(start..hints.len());
    }
    let top = ui.cursor().top();
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = CAP_GAP;
        for line in lines {
            let gaps = HINT_GAP * line.len().saturating_sub(1) as f32;
            let line_w = widths[line.clone()].iter().sum::<f32>() + gaps;
            ui.allocate_ui_with_layout(
                egui::vec2(line_w, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = CAP_GAP;
                    for (i, hint) in hints[line].iter().enumerate() {
                        if i > 0 {
                            ui.add_space(HINT_GAP - CAP_GAP);
                        }
                        add_hint(ui, hint);
                    }
                },
            );
        }
    });
    ui.cursor().top() - top
}

/// One hint's badges, then its words, which never wrap.
fn add_hint(ui: &mut egui::Ui, (keys, action): &Hint) {
    for key in *keys {
        key_cap(ui, key, HINT_FONT);
    }
    let text = egui::RichText::new(*action).color(DIM).size(HINT_FONT);
    ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
}

/// The width [`add_hint`] takes.
fn hint_width(ui: &egui::Ui, (keys, action): &Hint) -> f32 {
    let caps: f32 = keys
        .iter()
        .map(|key| cap_size(ui, key, HINT_FONT).x + CAP_GAP)
        .sum();
    caps + text_width(ui, action, HINT_FONT)
}

fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    let font = egui::FontId::proportional(size);
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, DIM).size().x)
}
