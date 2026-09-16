//! Rendering of Game Mode's profile screens (state in
//! [`crate::overlay::game_profiles`]): the list of profiles, one profile's own
//! rows, and the confirmation over a removal — each over the shared panel
//! chrome, so a long list scrolls the way the menu's and settings' do.

use super::panel::{self, center_selected, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{ACCENT, DIM, ROW_FONT};
use crate::app::{AppCommand, GameProfilesAction};
use crate::overlay::game_profiles::{GameProfiles, ProfileAction};
use egui_sdl2::egui;

/// Row height, matching the settings overlay's field rows.
const ROW_H: f32 = 30.0;

pub(super) fn add_game_profiles(
    ctx: &egui::Context,
    screens: &GameProfiles,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let width = screen.width() - SIDES;
    let closed = panel::panel(ctx, "game_profiles", screen, |ui| {
        let (title, hint) = header(screens);
        ui.label(
            egui::RichText::new(title)
                .color(ACCENT)
                .size(ROW_FONT)
                .strong(),
        );
        ui.label(egui::RichText::new(hint).color(DIM).size(ROW_FONT));
        ui.add_space(ROW_GAP * 2.0);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        let rows = rows(screens);
        section_scroll(ui, screen).show(ui, |ui| {
            for (index, (label, value)) in rows.into_iter().enumerate() {
                let selected = index == screens.selected();
                let resp = add_row(ui, width, selected, &label, &value);
                if selected {
                    center_selected(&resp);
                }
                if resp.clicked() {
                    commands.push(AppCommand::GameProfiles(GameProfilesAction::Click(index)));
                }
            }
        });
    });
    if closed {
        commands.push(AppCommand::GameProfiles(GameProfilesAction::Close));
    }
}

/// The panel's title and the line under it, both worded for the screen that is
/// up: the list, one profile, or the question over it.
fn header(screens: &GameProfiles) -> (String, &'static str) {
    let Some(row) = screens.open_row() else {
        // Plural of the menu row that opens it: this is the list of them.
        return (
            "INPUT PROFILES".to_string(),
            "A opens a profile, B goes back",
        );
    };
    let name = row.name.to_uppercase();
    match screens.confirming() {
        // The question names the removal it is asking about, since the answer
        // differs: a built-in comes back, anything else is gone.
        true => (
            format!("{} {name}?", remove_label(screens).to_uppercase()),
            "A takes it",
        ),
        false => (name, "A takes the row, B goes back"),
    }
}

/// What this profile's removal row says — the question and the row itself have
/// to agree.
fn remove_label(screens: &GameProfiles) -> &'static str {
    screens
        .open_row()
        .and_then(|row| row.remove)
        .map_or("Remove", ProfileAction::label)
}

/// The rows of whichever screen is up, as label and trailing value.
fn rows(screens: &GameProfiles) -> Vec<(String, String)> {
    if screens.confirming() {
        return vec![
            (remove_label(screens).to_string(), String::new()),
            ("Cancel".to_string(), String::new()),
        ];
    }
    if screens.open_row().is_none() {
        return screens
            .rows()
            .iter()
            .map(|row| {
                let value = match row.in_use {
                    true => "in use",
                    false => "",
                };
                (row.name.clone(), value.to_string())
            })
            .collect();
    }
    // While the keyboard is up, the row it was opened from carries what is
    // being typed — there is nowhere else on screen the name would show.
    let naming = screens.naming();
    screens
        .actions()
        .into_iter()
        .map(|action| {
            let typed = naming.filter(|naming| match action {
                ProfileAction::Rename => !naming.copy,
                ProfileAction::Duplicate => naming.copy,
                _ => false,
            });
            let value = typed.map(|naming| naming.text.clone()).unwrap_or_default();
            (action.label().to_string(), value)
        })
        .collect()
}

/// One row: the label on the left, its value on the right — the same shape as
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
