//! Shared chrome for the menu and settings overlays: panel metrics, the pinned
//! frame with its close button, the section tab bar, the bounded section scroll.

use super::theme::{close_button, CLOSE_SIZE, PANEL_FILL, ROW_FONT};
use egui_sdl2::egui;

/// Shared row radius and gap so both overlays read alike (row height stays
/// per-overlay).
pub(super) const ROW_RADIUS: f32 = 6.0;
pub(super) const ROW_GAP: f32 = 4.0;

/// Panel inner padding; the sides get more room than the top and bottom.
/// [`SIDES`] is the pair, subtracted from the screen width for row widths.
pub(super) const PAD_X: f32 = 30.0;
pub(super) const PAD_Y: f32 = 16.0;
pub(super) const SIDES: f32 = PAD_X * 2.0;

/// Section-bar tab height.
const TAB_H: f32 = 28.0;

/// The full-screen panel shell. `constrain(false)`: the frame fills the screen
/// exactly, so an egui "fit" shift would cancel the left padding. The close
/// button is painted outside the content flow so it can't shift it (B also
/// closes). Returns whether it was clicked.
pub(super) fn panel(
    ctx: &egui::Context,
    id: &str,
    screen: egui::Rect,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut closed = false;
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(PANEL_FILL)
                .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
                .show(ui, |ui| {
                    ui.set_min_size(screen.size() - egui::vec2(SIDES, PAD_Y * 2.0));
                    let close_rect = egui::Rect::from_min_size(
                        egui::pos2(screen.right() - PAD_X - CLOSE_SIZE, screen.top() + PAD_Y),
                        egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
                    );
                    closed = close_button(ui, close_rect, egui::Id::new((id, "close"))).clicked();
                    add_contents(ui);
                });
        });
    closed
}

/// The top section bar: a selectable tab per section, with `trailing` laid out
/// right-to-left in the room left of the tabs. Returns the clicked section.
pub(super) fn section_bar<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    sections: impl IntoIterator<Item = T>,
    active: T,
    label: fn(T) -> &'static str,
    trailing: impl FnOnce(&mut egui::Ui),
) -> Option<T> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        // The gap turns the flush button row into a segmented control.
        ui.spacing_mut().item_spacing.x = 6.0;
        for section in sections {
            let tab = egui::Button::selectable(
                section == active,
                egui::RichText::new(label(section))
                    .color(egui::Color32::WHITE)
                    .size(ROW_FONT),
            )
            .corner_radius(ROW_RADIUS)
            .min_size(egui::vec2(0.0, TAB_H));
            if ui.add(tab).clicked() {
                clicked = Some(section);
            }
        }
        // Width from `screen`; `available_width()` runs past the visible edge.
        // Reserve the close button's footprint so `trailing` sits left of it.
        let remaining = screen.width() - SIDES - ui.min_rect().width() - (CLOSE_SIZE + 8.0);
        ui.allocate_ui_with_layout(
            egui::vec2(remaining.max(1.0), TAB_H),
            egui::Layout::right_to_left(egui::Align::Center),
            trailing,
        );
    });
    clicked
}

/// A section's scroll area, capped to the room down to the screen bottom: the
/// panel's `Area` auto-sizes, so an unbounded `ScrollArea` would grow past the
/// screen and clip instead of scrolling. Callers pair it with `scroll_to_me`.
pub(super) fn section_scroll(ui: &egui::Ui, screen: egui::Rect) -> egui::ScrollArea {
    let max_h = (screen.bottom() - PAD_Y - ui.cursor().top()).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(max_h)
}
