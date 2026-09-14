//! Rendering of the Game Mode menu (state lives in
//! [`crate::overlay::game_menu`]): a centered panel over the running game.
//! Up/Down move, A / Left/Right act on the focused row, B resumes.

use super::panel::{ROW_GAP, ROW_RADIUS};
use super::theme::{ACCENT, PANEL_FILL, ROW_FONT};
use crate::app::{AppCommand, GameMenuAction};
use crate::config::GameProfile;
use crate::overlay::game_menu::{GameMenu, GameRow};
use egui_sdl2::egui;

/// Row height, matching the settings overlay's field rows.
const ROW_H: f32 = 30.0;

/// Panel width where the screen has room for it; a 640px panel gets the fallback
/// below (the whole width less a margin).
const PANEL_W: f32 = 340.0;

/// Margin left either side of the panel on a screen too narrow for [`PANEL_W`].
const SIDE_MARGIN: f32 = 48.0;

pub(super) fn add_game_menu(
    ctx: &egui::Context,
    menu: &GameMenu,
    profile: GameProfile,
    in_game_mode: bool,
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
            egui::Frame::default()
                .fill(PANEL_FILL)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(0x55)))
                .corner_radius(10.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    add_header(ui);
                    ui.spacing_mut().item_spacing.y = ROW_GAP;
                    for (index, row) in GameRow::ALL.into_iter().enumerate() {
                        let value = match row {
                            GameRow::Profile => profile.label(),
                            _ => "",
                        };
                        let label = row.label(in_game_mode);
                        let resp = add_row(ui, width, index == menu.selected(), label, value);
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

/// One row: the label, and a value pushed to the trailing edge — the same shape
/// as the settings rows, so the highlight reads identically.
fn add_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    label: &str,
    value: &str,
) -> egui::Response {
    let label = egui::RichText::new(label)
        .color(egui::Color32::WHITE)
        .size(ROW_FONT);
    let value = egui::RichText::new(value).color(ACCENT).size(ROW_FONT);
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (label, egui::Atom::grow(), value))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}
