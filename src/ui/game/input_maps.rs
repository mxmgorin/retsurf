//! Rendering of Game Mode's map screens (state in
//! [`crate::overlay::game::input_maps`]): the list of maps, one map's own
//! rows, and the confirmation over a removal — each over the shared panel
//! chrome, so a long list scrolls the way the menu's and settings' do.

use crate::command::{AppCommand, GameInputMapsAction};
use crate::overlay::game::input_maps::{InputMaps, MapAction, NameFor, NEW_MAP_LABEL};
use crate::ui::panel;
use egui_sdl2::egui;

pub(in crate::ui) fn add_input_maps(
    ctx: &egui::Context,
    screens: &InputMaps,
    commands: &mut Vec<AppCommand>,
) {
    let closed = panel::row_list(
        ctx,
        "input_maps",
        &title(screens),
        rows(screens),
        screens.selected(),
        |index| commands.push(AppCommand::GameInputMaps(GameInputMapsAction::Click(index))),
    );
    if closed {
        commands.push(AppCommand::GameInputMaps(GameInputMapsAction::Close));
    }
}

/// The panel's title, worded for the screen that is up: the list, one map,
/// or the question over it. Nothing under it — every verb here is a row, so a
/// hint could only name them a second time.
fn title(screens: &InputMaps) -> String {
    let Some(row) = screens.open_row() else {
        // Plural of the menu row that opens it: this is the list of them.
        return "INPUT MAPS".to_string();
    };
    let name = row.name.to_uppercase();
    match screens.confirming() {
        // The question names the removal it is asking about, since the answer
        // differs: a built-in comes back, anything else is gone.
        true => format!("{} {name}?", remove_label(screens).to_uppercase()),
        false => name,
    }
}

/// What this map's removal row says; the question over it must agree.
fn remove_label(screens: &InputMaps) -> &'static str {
    screens
        .open_row()
        .and_then(|row| row.remove)
        .map_or("Remove", MapAction::label)
}

/// The rows of whichever screen is up.
fn rows(screens: &InputMaps) -> Vec<(String, String)> {
    if screens.confirming() {
        return vec![
            (remove_label(screens).to_string(), String::new()),
            ("Cancel".to_string(), String::new()),
        ];
    }
    // While the keyboard is up, the row it was opened from carries what is
    // being typed — there is nowhere else on screen the name would show.
    let naming = screens.naming();
    if screens.open_row().is_none() {
        let typed = naming.filter(|naming| naming.what == NameFor::New);
        let new = (
            NEW_MAP_LABEL.to_string(),
            typed.map(|naming| naming.text.clone()).unwrap_or_default(),
        );
        return std::iter::once(new)
            .chain(screens.rows().iter().map(|row| {
                let value = match row.in_use {
                    true => "in use",
                    false => "",
                };
                (row.name.clone(), value.to_string())
            }))
            .collect();
    }
    screens
        .actions()
        .into_iter()
        .map(|action| {
            let typed = naming.filter(|naming| match action {
                MapAction::Rename => naming.what == NameFor::Rename,
                MapAction::Duplicate => naming.what == NameFor::Duplicate,
                _ => false,
            });
            let value = typed.map(|naming| naming.text.clone()).unwrap_or_default();
            (action.label().to_string(), value)
        })
        .collect()
}
