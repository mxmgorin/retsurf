//! Rendering of the Game Mode map editor (state in
//! [`crate::overlay::game::map_edit`]): a row per bound source with the row that
//! adds one after them, and the list of what the focused row can send — both
//! over the shared panel chrome, so a long list scrolls the way the menu's and
//! settings' do.

use crate::command::{AppCommand, GameMapEditAction};
use crate::overlay::game::map_edit::{Device, Kind, MapEdit, MouseKind, Slot, ADD_ROW};
use crate::ui::panel::{self, ListItem};
use egui_sdl2::egui;
use inputbind::Pad;

pub(in crate::ui) fn add_map_edit(
    ctx: &egui::Context,
    edit: &MapEdit,
    commands: &mut Vec<AppCommand>,
) {
    let closed = panel::grouped_row_list(
        ctx,
        "map_edit",
        &title(edit),
        items(edit),
        edit.selected(),
        |index| commands.push(AppCommand::GameMapEdit(GameMapEditAction::Click(index))),
    );
    if closed {
        commands.push(AppCommand::GameMapEdit(GameMapEditAction::Close));
    }
}

/// The panel's title, worded for what the screen is doing. Nothing under it —
/// every verb here is a row, so a hint could only name them a second time.
fn title(edit: &MapEdit) -> String {
    if edit.capturing() {
        return "PRESS A BUTTON OR KEY, OR PUSH A STICK".to_string();
    }
    if edit.kind_open() {
        let slot = edit.slot().map(|slot| slot.name()).unwrap_or_default();
        return match edit.device_open() {
            Some(device) => format!("{} SENDS {}", slot.to_uppercase(), device.title()),
            None => format!("{} SENDS", slot.to_uppercase()),
        };
    }
    edit.map_name().to_uppercase()
}

/// Whichever list is up. The sources arrive grouped by the table that holds
/// them, so a heading goes in wherever the group changes; the row that listens
/// ends the list, carrying whatever the last attempt had to say.
fn items(edit: &MapEdit) -> Vec<ListItem> {
    if edit.kind_open() {
        let Some(slot) = edit.slot() else {
            return vec![];
        };
        let labels: Vec<String> = match edit.device_open() {
            Some(Device::Mouse) => MouseKind::all(&slot)
                .iter()
                .map(|kind| kind.label().to_string())
                .collect(),
            // The buttons as the pad's own table names them, so the list and
            // the file agree on what was picked.
            Some(Device::Gamepad) => Pad::ALL.iter().map(|pad| pad.name().to_string()).collect(),
            None => Kind::all(&slot)
                .iter()
                .map(|kind| kind.label().to_string())
                .collect(),
        };
        return labels
            .into_iter()
            .map(|label| ListItem::Row(label, String::new()))
            .collect();
    }
    let mut items = vec![];
    let mut group = None;
    for row in edit.rows() {
        let heading = heading_for(&row.slot);
        if group != Some(heading) {
            items.push(ListItem::Heading(heading.to_string()));
            group = Some(heading);
        }
        items.push(ListItem::Row(row.slot.label(), row.target.clone()));
    }
    let note = edit.note().unwrap_or_default().to_string();
    items.push(ListItem::Row(ADD_ROW.to_string(), note));
    items
}

/// The heading a source sits under — the table it is written in, said as the
/// screen says things.
fn heading_for(slot: &Slot) -> &'static str {
    match slot {
        Slot::Stick(_) | Slot::Direction(..) => "STICKS",
        Slot::Button(_) => "GAMEPAD",
        Slot::Key(_) => "KEYBOARD",
    }
}
