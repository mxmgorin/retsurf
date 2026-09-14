//! Rendering of the Game Mode profile editor (state in
//! [`crate::overlay::game_edit`]): one row per pad over the shared panel chrome,
//! so a long list scrolls the way the menu's and settings' do.

use super::panel::{self, center_selected, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{ACCENT, DIM, ROW_FONT};
use crate::app::{AppCommand, GameEditAction};
use crate::overlay::game_edit::{sources, GameEdit, Kind};
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
        let pad = edit.source();
        let (title, hint) = match edit.kind_open() {
            Some(_) => (format!("{} SENDS", pad.name().to_uppercase()), "A takes it"),
            None => (
                format!("GAME MODE PROFILE - {profile_name}"),
                "A opens a button, B saves",
            ),
        };
        ui.label(
            egui::RichText::new(title)
                .color(ACCENT)
                .size(ROW_FONT)
                .strong(),
        );
        ui.label(egui::RichText::new(hint).color(DIM).size(ROW_FONT));
        ui.add_space(ROW_GAP * 2.0);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        match edit.kind_open() {
            Some(at) => add_kinds(ui, screen, width, at, commands),
            None => add_rows(ui, screen, width, edit, targets, commands),
        }
    });
    if closed {
        commands.push(AppCommand::GameEdit(GameEditAction::Close));
    }
}

/// The rows: one per pad, with what it sends.
fn add_rows(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    width: f32,
    edit: &GameEdit,
    targets: &[String],
    commands: &mut Vec<AppCommand>,
) {
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
}

/// What the focused row can be, all of it on screen — the list is the whole
/// vocabulary, so nothing here needs a verb the screen cannot show.
fn add_kinds(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    width: f32,
    at: usize,
    commands: &mut Vec<AppCommand>,
) {
    section_scroll(ui, screen).show(ui, |ui| {
        for (index, kind) in Kind::ALL.into_iter().enumerate() {
            let resp = add_row(ui, width, index == at, kind.label(), "");
            if index == at {
                center_selected(&resp);
            }
            if resp.clicked() {
                commands.push(AppCommand::GameEdit(GameEditAction::Activate));
            }
        }
    });
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
