//! Rendering of the link-hint overlay (state lives in [`crate::overlay::hints`]): a thin
//! accent frame on every clickable element, with the selected one filled and
//! outlined brighter.

use super::theme::ACCENT;
use crate::overlay::hints::Hints;
use egui_sdl2::egui;

pub(super) fn add_hints(ctx: &egui::Context, hints: &Hints, webview_top: f32) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("hints"),
    ));
    let accent = ACCENT;
    let frame = egui::Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 110);
    let selected_fill =
        egui::Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 45);

    for (i, hint) in hints.hints().iter().enumerate() {
        // Rects are viewport-relative; shift below the toolbar.
        let rect = egui::Rect::from_min_size(
            egui::pos2(hint.x, hint.y + webview_top),
            egui::vec2(hint.w, hint.h),
        );
        if i == hints.selected() {
            painter.rect(
                rect.expand(2.0),
                3.0,
                selected_fill,
                egui::Stroke::new(2.0, accent),
                egui::StrokeKind::Outside,
            );
        } else {
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, frame),
                egui::StrokeKind::Outside,
            );
        }
    }
}
