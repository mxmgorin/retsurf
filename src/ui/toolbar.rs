//! The top toolbar: navigation buttons, the address bar, bookmark toggle, and
//! the chips that jump into menu sections (tab position, active downloads).

use crate::app::{AppCommand, MenuAction};
use crate::browser::{BrowserCommand, BrowserState};
use crate::overlay::menu::Section;
use egui_sdl2::egui::{self, Vec2};

/// Create a frameless button with square sizing, as used in the toolbar.
#[inline]
fn new_toolbar_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: 20.0, y: 20.0 })
}

#[inline]
fn new_text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).id(egui::Id::new(id))
}

#[inline]
fn is_key_pressed(ui: &mut egui::Ui, response: egui::Response, key: egui::Key) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(key))
}

#[inline]
pub(super) fn add_toolbar(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    // 1-based active tab index and total tab count, e.g. `(2, 3)` → "2/3".
    tab_pos: (usize, usize),
    // Downloads still in flight; shown as a `⬇N` chip that jumps to the section.
    active_downloads: usize,
    // Active tab's page zoom percent when off the config default (chip hidden at it).
    zoom_pct: Option<u16>,
) {
    let frame = egui::Frame::default()
        .fill(ui.style().visuals.window_fill)
        .inner_margin(4.0);
    egui::Panel::top("toolbar")
        .frame(frame)
        .show_inside(ui, |ui| {
            ui.allocate_ui_with_layout(
                ui.available_size(),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui.add(new_toolbar_button("⏴")).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Back));
                    }
                    if ui.add(new_toolbar_button("⏵")).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Foward));
                    }

                    if state.is_loading() {
                        ui.add(new_toolbar_button("X"));
                    } else {
                        if ui.add(new_toolbar_button("↻")).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Reload));
                        }
                    }

                    ui.add_space(2.0);
                    // The bookmark icons sit at the right edge; the address bar fills
                    // the gap between them and the navigation buttons. ★ toggles the
                    // current page (filled when saved); ☰ opens the menu.
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.add(new_toolbar_button("☰")).clicked() {
                                commands.push(AppCommand::Menu(MenuAction::Open));
                            }
                            // ⬇ U+2B07 (not ↓ U+2193): egui's default fonts lack the
                            // plain arrow, only the emoji one renders.
                            if active_downloads > 0 {
                                let label = format!("⬇{active_downloads}");
                                if ui.add(new_toolbar_button(&label)).clicked() {
                                    commands.push(AppCommand::Menu(MenuAction::Open));
                                    commands.push(AppCommand::Menu(MenuAction::SetSection(
                                        Section::Downloads,
                                    )));
                                }
                            }
                            // Active tab position, bracketed (e.g. "[2/3]") beside the
                            // menu button — a full border would read as a selection.
                            // Shown only with multiple tabs; clicking it opens the
                            // menu's Tabs section (like the ⬇ chip for downloads).
                            if tab_pos.1 > 1 {
                                let label = format!("[{}/{}]", tab_pos.0, tab_pos.1);
                                if ui.add(new_toolbar_button(&label)).clicked() {
                                    commands.push(AppCommand::Menu(MenuAction::Open));
                                    commands.push(AppCommand::Menu(MenuAction::SetSection(
                                        Section::Tabs,
                                    )));
                                }
                            }
                            // Page-zoom chip (e.g. "125%"), shown only while the
                            // active tab is off the config default; clicking resets.
                            if let Some(pct) = zoom_pct {
                                let label = format!("{pct}%");
                                if ui.add(new_toolbar_button(&label)).clicked() {
                                    commands.push(AppCommand::Browser(BrowserCommand::Zoom(0)));
                                }
                            }
                            if ui
                                .add(new_toolbar_button(if bookmarked { "★" } else { "☆" }))
                                .clicked()
                            {
                                commands.push(AppCommand::ToggleBookmark);
                            }
                            // 🖹 "document with text" — the page-with-lines reader
                            // glyph; lives in egui's emoji-icon-font (cmap-checked;
                            // most other reader-ish glyphs are tofu).
                            if ui.add(new_toolbar_button("🖹")).clicked() {
                                commands.push(AppCommand::Browser(BrowserCommand::Reader));
                            }
                            let location = ui.add_sized(
                                ui.available_size(),
                                new_text_edit(state.get_location_mut(), "location"),
                            );
                            if is_key_pressed(ui, location, egui::Key::Enter) {
                                commands.push(AppCommand::Browser(BrowserCommand::Load));
                            }
                        },
                    );
                },
            );
        });
}
