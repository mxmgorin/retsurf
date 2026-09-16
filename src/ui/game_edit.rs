//! Rendering of the Game Mode profile editor (state in
//! [`crate::overlay::game_edit`]): one row per source, one stick's own rows, and
//! the list of what the focused row can send — each over the shared panel
//! chrome, so a long list scrolls the way the menu's and settings' do.

use super::panel::{self, center_selected, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{ACCENT, ROW_FONT};
use crate::app::{AppCommand, GameEditAction};
use crate::overlay::game_edit::{sources, GameEdit, Kind, Source, StickRow};
use egui_sdl2::egui;

/// Row height, matching the settings overlay's field rows.
const ROW_H: f32 = 30.0;

/// What an unbound source reads as.
const UNBOUND: &str = "-";

pub(super) fn add_game_edit(ctx: &egui::Context, edit: &GameEdit, commands: &mut Vec<AppCommand>) {
    let screen = ctx.content_rect();
    let width = screen.width() - SIDES;
    let closed = panel::panel(ctx, "game_edit", screen, |ui| {
        ui.label(
            egui::RichText::new(title(edit))
                .color(ACCENT)
                .size(ROW_FONT)
                .strong(),
        );
        ui.add_space(ROW_GAP * 3.0);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        let rows = rows(edit);
        section_scroll(ui, screen).show(ui, |ui| {
            for (index, (label, value)) in rows.into_iter().enumerate() {
                let selected = index == edit.selected();
                let resp = add_row(ui, width, selected, &label, &value);
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

/// The panel's title, worded for the list that is up. Nothing under it — every
/// verb here is a row, so a hint could only name them a second time.
fn title(edit: &GameEdit) -> String {
    if edit.kind_open() {
        let slot = edit.slot().map(|slot| slot.name()).unwrap_or_default();
        return format!("{} SENDS", slot.to_uppercase());
    }
    match edit.stick_open() {
        Some(side) => format!("STICK.{} - {}", side.name(), edit.profile_name()).to_uppercase(),
        None => format!(
            "BUTTONS AND STICKS - {}",
            edit.profile_name().to_uppercase()
        ),
    }
}

/// The rows of whichever list is up.
fn rows(edit: &GameEdit) -> Vec<(String, String)> {
    if edit.kind_open() {
        let kinds = edit.slot().map(Kind::all).unwrap_or_default();
        return kinds
            .iter()
            .map(|kind| (kind.label().to_string(), String::new()))
            .collect();
    }
    let targets = edit.targets();
    let Some(side) = edit.stick_open() else {
        return sources()
            .into_iter()
            .map(|source| {
                let value = match source {
                    Source::Button(pad) => targets
                        .pads
                        .get(pad as usize)
                        .cloned()
                        .unwrap_or_else(|| UNBOUND.to_string()),
                    Source::Stick(side) => targets.sticks[side as usize].role.clone(),
                };
                (source.name(), value)
            })
            .collect();
    };
    let stick = &targets.sticks[side as usize];
    edit.stick_rows()
        .into_iter()
        .map(|row| match row {
            StickRow::Sends => ("sends".to_string(), stick.role.clone()),
            StickRow::Direction(dir) => (dir.name().to_string(), stick.dirs[dir as usize].clone()),
        })
        .collect()
}

/// The shape the settings rows use, so the highlight reads identically.
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
