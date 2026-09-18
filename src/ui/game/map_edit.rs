//! Rendering of the Game Mode map editor (state in
//! [`crate::overlay::game::map_edit`]): one row per source, one stick's own rows, and
//! the list of what the focused row can send — each over the shared panel
//! chrome, so a long list scrolls the way the menu's and settings' do.

use crate::app::{AppCommand, GameMapEditAction};
use crate::overlay::game::map_edit::{sources, Kind, MapEdit, Source, StickRow, UNBOUND};
use crate::ui::panel;
use egui_sdl2::egui;

pub(in crate::ui) fn add_map_edit(
    ctx: &egui::Context,
    edit: &MapEdit,
    commands: &mut Vec<AppCommand>,
) {
    let closed = panel::row_list(
        ctx,
        "map_edit",
        &title(edit),
        rows(edit),
        edit.selected(),
        |index| commands.push(AppCommand::GameMapEdit(GameMapEditAction::Click(index))),
    );
    if closed {
        commands.push(AppCommand::GameMapEdit(GameMapEditAction::Close));
    }
}

/// The panel's title, worded for the list that is up. Nothing under it — every
/// verb here is a row, so a hint could only name them a second time.
fn title(edit: &MapEdit) -> String {
    if edit.kind_open() {
        let slot = edit.slot().map(|slot| slot.name()).unwrap_or_default();
        return format!("{} SENDS", slot.to_uppercase());
    }
    match edit.stick_open() {
        Some(side) => format!("STICK.{} - {}", side.name(), edit.map_name()).to_uppercase(),
        None => format!("BUTTONS AND STICKS - {}", edit.map_name().to_uppercase()),
    }
}

/// The rows of whichever list is up.
fn rows(edit: &MapEdit) -> Vec<(String, String)> {
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
