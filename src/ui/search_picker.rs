//! The search-engine picker: the search glyph of the address bar and of the
//! start page's field as a button that opens the engine list beside it.

use super::panel::ROW_H;
use super::theme::{palette, ROW_FONT};
use crate::command::AppCommand;
use crate::config::BrowserConfig;
use egui_sdl2::egui;

/// The engines the picker offers, the one in use, and what an empty field
/// shows for it.
#[derive(Default)]
pub(super) struct SearchEngines {
    names: Vec<String>,
    in_use: Option<usize>,
    /// What an empty address or search field shows, naming the engine in use.
    pub hint: String,
}

impl SearchEngines {
    pub(super) fn new(browser: &BrowserConfig) -> Self {
        let in_use = browser.search_index();
        let name = in_use.map_or("", |i| browser.search_engines[i].name.as_str());
        Self {
            names: browser
                .search_engines
                .iter()
                .map(|e| e.name.clone())
                .collect(),
            in_use,
            hint: format!("Search {name} or enter address"),
        }
    }
}

/// `glyph` in `font` as a button whose click opens the engine list beside it;
/// a pick pushes [`AppCommand::UseSearchEngine`]. Returns whether either was
/// clicked, which takes focus off a field being edited.
pub(super) fn search_button(
    ui: &mut egui::Ui,
    glyph: &str,
    font: egui::FontId,
    engines: &SearchEngines,
    commands: &mut Vec<AppCommand>,
) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(glyph.to_string(), font, egui::Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let color = match resp.hovered() || open {
        true => palette().accent,
        false => palette().muted,
    };
    ui.painter().galley(rect.min, galley, color);
    // The larger side's room, a row short for the frame: the list fits on
    // one side of the glyph and scrolls past that.
    let screen = ui.ctx().content_rect();
    let room = (screen.bottom() - rect.bottom()).max(rect.top() - screen.top()) - ROW_H;
    let mut picked = false;
    egui::Popup::menu(&resp).show(|ui| {
        egui::ScrollArea::vertical()
            .max_height(room)
            .show(ui, |ui| {
                for (i, name) in engines.names.iter().enumerate() {
                    let name = egui::RichText::new(name).size(ROW_FONT);
                    let row = egui::Button::selectable(engines.in_use == Some(i), name)
                        .min_size(egui::vec2(0.0, ROW_H));
                    if ui.add(row).clicked() {
                        commands.push(AppCommand::UseSearchEngine(i));
                        picked = true;
                    }
                }
            });
    });
    resp.clicked() || picked
}
