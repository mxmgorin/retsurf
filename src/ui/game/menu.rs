//! Rendering of the Game Mode menu (state lives in
//! [`crate::overlay::game::menu`]): a centered panel over the running game.
//! Up/Down move, A / Left/Right act on the focused row, B resumes.

use crate::command::{AppCommand, GameMenuAction};
use crate::overlay::game::menu::{GameMenu, GameRow};
use crate::ui::panel::{self, ROW_GAP};
use crate::ui::theme::{self, ACCENT, ROW_FONT};
use egui_sdl2::egui;

/// Panel width where the screen has room for it; a 640px panel gets the fallback
/// below (the whole width less a margin).
const PANEL_W: f32 = 340.0;

/// Margin left either side of the panel on a screen too narrow for [`PANEL_W`].
const SIDE_MARGIN: f32 = 48.0;

/// Opacity of the panel's fill, so the game still shows through behind the rows.
const PANEL_OPACITY: f32 = 0.8;

pub(in crate::ui) fn add_game_menu(
    ctx: &egui::Context,
    menu: &GameMenu,
    map_name: &str,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let width = (screen.width() - SIDE_MARGIN).min(PANEL_W);
    egui::Area::new(egui::Id::new("game_menu"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            // The game keeps running behind this (Servo has no suspend API), so
            // dim it — in this Area's own layer, painted before the panel.
            ui.painter().with_clip_rect(screen).rect_filled(
                screen,
                0.0,
                egui::Color32::from_black_alpha(160),
            );
            let fill = theme::PANEL_FILL.gamma_multiply(PANEL_OPACITY);
            theme::card_frame().fill(fill).show(ui, |ui| {
                ui.set_max_width(width);
                add_header(ui);
                ui.spacing_mut().item_spacing.y = ROW_GAP;
                for (index, &row) in menu.rows().iter().enumerate() {
                    let value = match row {
                        GameRow::InputMap => map_name,
                        GameRow::View => menu.scaling.label(),
                        _ => "",
                    };
                    let label = row.label();
                    let resp = panel::row(ui, width, index == menu.selected(), label, value);
                    if resp.clicked() {
                        commands.push(AppCommand::GameMenu(GameMenuAction::Click(index)));
                    }
                }
            });
        });
}

/// The panel's title, so the rows below it read as Game Mode's and not a page's.
fn add_header(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("GAME MODE")
            .color(ACCENT)
            .size(ROW_FONT)
            .strong(),
    );
    ui.add_space(ROW_GAP * 2.0);
}
