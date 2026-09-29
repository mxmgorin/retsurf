//! Rendering of the edge strips (state lives in [`crate::overlay::quick_access`]):
//! Quick Access at the right edge and Quick Menu at the left, each full height
//! with no backdrop dim, so the page or game beside it shows what a quick row
//! just changed. Up/Down move, A / Left/Right act on the focused row, B closes.

use crate::command::{AppCommand, QuickAccessAction};
use crate::overlay::quick_access::{QuickAccess, Strip};
use crate::ui::panel::{self, ROW_GAP};
use crate::ui::theme::{self, ACCENT, ROW_FONT};
use egui_sdl2::egui;

/// The strip's width where the screen has room for it; a narrower screen gets
/// [`MAX_SHARE`] of its width.
const STRIP_W: f32 = 280.0;

/// The most of the screen's width the strip may cover.
const MAX_SHARE: f32 = 0.5;

/// Opacity of the strip's fill: enough to read rows over any page.
const STRIP_OPACITY: f32 = 0.92;

pub(in crate::ui) fn add_quick_access(
    ctx: &egui::Context,
    panel: &QuickAccess,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let frame = theme::card_frame()
        .fill(theme::PANEL_FILL.gamma_multiply(STRIP_OPACITY))
        .corner_radius(0.0);
    let margin = frame.inner_margin.sum();
    let outer = STRIP_W.min(screen.width() * MAX_SHARE);
    let width = outer - margin.x;
    // Each on the side of the button that opens it.
    let (left, title) = match panel.strip() {
        Strip::QuickAccess => (screen.right() - outer, "QUICK ACCESS"),
        Strip::QuickMenu => (screen.left(), "QUICK MENU"),
    };
    egui::Area::new(egui::Id::new("quick_access"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(left, screen.top()))
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.set_min_height(screen.height() - margin.y);
                add_header(ui, title);
                ui.spacing_mut().item_spacing.y = ROW_GAP;
                for index in 0..panel.rows().len() {
                    let selected = index == panel.selected();
                    let label = panel.label(index);
                    let resp = panel::named_row(ui, width, selected, label, panel.value(index));
                    if resp.clicked() {
                        commands.push(AppCommand::QuickAccess(QuickAccessAction::Click(index)));
                    }
                }
            });
        });
}

/// The strip's title, one per strip in either mode: the rows say which is on.
fn add_header(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .color(ACCENT)
            .size(ROW_FONT)
            .strong(),
    );
    ui.add_space(ROW_GAP * 2.0);
}
