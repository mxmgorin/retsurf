//! Rendering of Quick Access (state lives in [`crate::overlay::quick_access`]):
//! a full-height strip at the left edge, with no backdrop dim, so the page or
//! game beside it shows what a quick row just changed. Up/Down move, A /
//! Left/Right act on the focused row, B closes.

use crate::command::{AppCommand, QuickAccessAction};
use crate::overlay::quick_access::QuickAccess;
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
    let width = STRIP_W.min(screen.width() * MAX_SHARE) - margin.x;
    egui::Area::new(egui::Id::new("quick_access"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.left_top())
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.set_min_height(screen.height() - margin.y);
                add_header(ui, panel.in_game_mode());
                ui.spacing_mut().item_spacing.y = ROW_GAP;
                for (index, row) in panel.rows().iter().enumerate() {
                    let selected = index == panel.selected();
                    let resp = panel::row(ui, width, selected, row.label(), panel.value(index));
                    if resp.clicked() {
                        commands.push(AppCommand::QuickAccess(QuickAccessAction::Click(index)));
                    }
                }
            });
        });
}

/// The strip's title: inside the mode it says the rows are Game Mode's.
fn add_header(ui: &mut egui::Ui, in_game_mode: bool) {
    let title = match in_game_mode {
        true => "GAME MODE",
        false => "QUICK ACCESS",
    };
    ui.label(
        egui::RichText::new(title)
            .color(ACCENT)
            .size(ROW_FONT)
            .strong(),
    );
    ui.add_space(ROW_GAP * 2.0);
}
