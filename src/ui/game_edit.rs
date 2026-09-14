//! Rendering of the Game Mode profile editor (state in
//! [`crate::overlay::game_edit`]): one row per pad over the shared panel chrome,
//! so a long list scrolls the way the menu's and settings' do.

use super::panel::{self, center_selected, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{ACCENT, DIM, ROW_FONT};
use crate::app::{AppCommand, GameEditAction};
use crate::overlay::game_edit::{sources, GameEdit};
use egui_sdl2::egui;

/// Row height, matching the settings overlay's field rows.
const ROW_H: f32 = 30.0;

pub(super) fn add_game_edit(
    ctx: &egui::Context,
    edit: &GameEdit,
    profile_name: &str,
    targets: &[String],
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let width = screen.width() - SIDES;
    let closed = panel::panel(ctx, "game_edit", screen, |ui| {
        ui.label(
            egui::RichText::new(format!("GAME MODE PROFILE - {profile_name}"))
                .color(ACCENT)
                .size(ROW_FONT)
                .strong(),
        );
        ui.label(
            egui::RichText::new("A picks a key, Left/Right steps the rest")
                .color(DIM)
                .size(ROW_FONT),
        );
        ui.add_space(ROW_GAP * 2.0);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        section_scroll(ui, screen).show(ui, |ui| {
            for (index, pad) in sources().into_iter().enumerate() {
                let selected = index == edit.selected();
                let value = targets.get(pad as usize).map_or("", String::as_str);
                let resp = add_row(ui, width, selected, pad.name(), value);
                if selected {
                    center_selected(&resp);
                }
                if resp.clicked() {
                    commands.push(AppCommand::GameEdit(GameEditAction::Click(index)));
                }
            }
        });
    });
    if closed {
        commands.push(AppCommand::GameEdit(GameEditAction::Close));
    }
}

/// One row: the pad on the left, what it sends on the right — the same shape as
/// the settings rows, so the highlight reads identically.
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
