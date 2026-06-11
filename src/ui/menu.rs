//! Rendering of the full-screen menu overlay (state lives in [`crate::overlay::menu`]):
//! the section bar with the ✖ close and contextual clear actions, and the four
//! section lists (Tabs / Bookmarks / History / Downloads).

use crate::app::{AppCommand, MenuAction};
use crate::browser::TabInfo;
use crate::data::history;
use crate::overlay::menu::{Menu, Section};
use egui_sdl2::egui;

/// Draw the full-screen menu overlay: a dark panel with a top section bar
/// (Tabs · Bookmarks · History · Downloads) over the active section's content,
/// plus a one-line control hint. Gamepad/keyboard: ◀▶ switch section, ▲▼ move,
/// A open, X delete, B close; the mouse can click a tab, a row, its ✖, or Close.
pub(super) fn add_menu(
    ctx: &egui::Context,
    menu: &Menu,
    tabs: &[TabInfo],
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let dim = egui::Color32::from_gray(0x99);
    egui::Area::new(egui::Id::new("menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_rgb(0x18, 0x18, 0x1c))
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_min_size(screen.size());

                    // Section bar (active tab highlighted), with a mouse-only ✖
                    // pinned to the top-right corner (the gamepad closes with B).
                    ui.horizontal(|ui| {
                        for section in Section::ALL {
                            let active = section == menu.section();
                            let tab = egui::Button::selectable(
                                active,
                                egui::RichText::new(section.label()).color(egui::Color32::WHITE),
                            );
                            if ui.add(tab).clicked() {
                                commands.push(AppCommand::Menu(MenuAction::SetSection(section)));
                            }
                        }
                        // Width from `screen`, not `available_width()`: the frame's
                        // min-size is screen + margins, so "available" runs past the
                        // visible right edge and would push the ✖ offscreen.
                        let remaining = screen.width() - 32.0 - ui.min_rect().width();
                        ui.allocate_ui_with_layout(
                            egui::vec2(remaining.max(26.0), 26.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Fixed square, like the rows' ✖ — auto-sizing pads
                                // the glyph unevenly.
                                if ui
                                    .add_sized(
                                        [26.0, 26.0],
                                        egui::Button::new(
                                            egui::RichText::new("✖")
                                                .color(egui::Color32::WHITE),
                                        ),
                                    )
                                    .clicked()
                                {
                                    commands.push(AppCommand::Menu(MenuAction::Close));
                                }
                                // The active section's bulk-clear action lives here
                                // (dim, mouse-only) instead of above its list, so
                                // the list starts at a stable height.
                                let clear_label = match menu.section() {
                                    Section::History
                                        if !menu.history().entries().is_empty() =>
                                    {
                                        Some("Clear all")
                                    }
                                    Section::Downloads if menu.downloads.has_finished() => {
                                        Some("Clear finished")
                                    }
                                    _ => None,
                                };
                                if let Some(label) = clear_label {
                                    if ui
                                        .add(egui::Button::new(
                                            egui::RichText::new(label).color(dim),
                                        ))
                                        .clicked()
                                    {
                                        commands.push(AppCommand::Menu(MenuAction::Clear));
                                    }
                                }
                            },
                        );
                    });
                    ui.label(
                        egui::RichText::new("⏴⏵ section   ⏶⏷ select   A open   X delete   B close")
                            .color(dim),
                    );
                    ui.add_space(8.0);

                    match menu.section() {
                        Section::Tabs => {
                            add_tabs_section(ui, screen, tabs, menu.tab_selected(), commands)
                        }
                        Section::Bookmarks => add_bookmarks_section(ui, screen, menu, dim, commands),
                        Section::History => add_history_section(ui, screen, menu, dim, commands),
                        Section::Downloads => add_downloads_section(ui, screen, menu, dim, commands),
                    }
                });
        });
}

/// Tabs section: the open tabs (active one marked) plus a trailing "+ New tab"
/// row. A row opens/switches, its ✖ closes, "+ New tab" opens a fresh tab.
fn add_tabs_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    tabs: &[TabInfo],
    selected: usize,
    commands: &mut Vec<AppCommand>,
) {
    let del_w = 26.0;
    let row_w = screen.width() - 32.0 - del_w - 6.0; // frame margins + delete + spacing
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, tab) in tabs.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.add_sized([del_w, 26.0], egui::Button::new("✖")).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::CloseTab(i)));
                }
                // The active (shown) tab stands out in the accent color and bold;
                // the cursor's row uses the selectable highlight, so the two are
                // distinguishable even on the same row.
                let text = if tab.active {
                    egui::RichText::new(&tab.title)
                        .color(egui::Color32::from_rgb(0x2f, 0x81, 0xf7))
                        .strong()
                } else {
                    egui::RichText::new(&tab.title).color(egui::Color32::WHITE)
                };
                let row = ui.add_sized(
                    [row_w, 26.0],
                    egui::Button::selectable(i == selected, text).truncate(),
                );
                if row.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenTab(i)));
                }
            });
        }
        // The "+ New tab" row sits at index `tabs.len()`.
        let row = ui.add_sized(
            [screen.width() - 32.0, 26.0],
            egui::Button::selectable(
                selected == tabs.len(),
                egui::RichText::new("+ New tab").color(egui::Color32::WHITE),
            ),
        );
        if row.clicked() {
            commands.push(AppCommand::Menu(MenuAction::NewTab));
        }
    });
}

/// Bookmarks section: the saved URLs, highlighted row selected.
fn add_bookmarks_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    menu: &Menu,
    dim: egui::Color32,
    commands: &mut Vec<AppCommand>,
) {
    let bookmarks = menu.bookmarks();
    if bookmarks.urls().is_empty() {
        ui.label(
            egui::RichText::new("No bookmarks yet — press ★ to add this page.").color(dim),
        );
        return;
    }

    // Fixed widths derived from the screen (not `ui.available_width()`, which is
    // unreliable inside a scroll area and made the list jump horizontally).
    let del_w = 26.0;
    let row_w = screen.width() - 32.0 - del_w - 6.0; // frame margins + delete + spacing
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, url) in bookmarks.urls().iter().enumerate() {
            let selected = i == bookmarks.selected();
            // ✖ deletes, the row opens (mouse); the gamepad uses the stick + A/X.
            ui.horizontal(|ui| {
                if ui.add_sized([del_w, 26.0], egui::Button::new("✖")).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
                let row = ui.add_sized(
                    [row_w, 26.0],
                    egui::Button::selectable(
                        selected,
                        egui::RichText::new(url).color(egui::Color32::WHITE),
                    )
                    .truncate(),
                );
                if row.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(url.clone())));
                }
            });
        }
    });
}

/// Downloads section: most-recent first, each row showing the file name and a
/// status (progress while active, size + date when done, the error otherwise).
/// ✖ cancels an active download or removes a finished entry (the file on disk is
/// kept); clicking a finished row opens the file in the browser. "Clear finished"
/// (in the menu's top bar) drops everything not in flight.
fn add_downloads_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    menu: &Menu,
    dim: egui::Color32,
    commands: &mut Vec<AppCommand>,
) {
    let downloads = &menu.downloads;
    if downloads.items().is_empty() {
        ui.label(egui::RichText::new("No downloads yet.").color(dim));
        return;
    }

    let del_w = 26.0;
    let status_w = 170.0; // fits "100% · 999.9 MB / 999.9 MB"-ish, truncated past that
    let row_w = screen.width() - 32.0 - del_w - status_w - 12.0;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, item) in downloads.items().iter().enumerate() {
            let selected = i == downloads.selected();
            ui.horizontal(|ui| {
                if ui.add_sized([del_w, 26.0], egui::Button::new("✖")).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
                let row = ui.add_sized(
                    [row_w, 26.0],
                    egui::Button::selectable(
                        selected,
                        egui::RichText::new(&item.filename).color(egui::Color32::WHITE),
                    )
                    .truncate(),
                );
                if row.clicked() {
                    if let Some(url) = downloads.open_url(i) {
                        commands.push(AppCommand::Menu(MenuAction::OpenUrl(url)));
                    }
                }
                ui.add_sized(
                    [status_w, 26.0],
                    egui::Label::new(egui::RichText::new(item.status_text()).color(dim))
                        .truncate(),
                );
            });
        }
    });
}

/// History section: visited URLs (most-recent first) with their visit date.
/// "Clear all" sits in the menu's top bar.
fn add_history_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    menu: &Menu,
    dim: egui::Color32,
    commands: &mut Vec<AppCommand>,
) {
    let hist = menu.history();
    if hist.entries().is_empty() {
        ui.label(egui::RichText::new("No history yet.").color(dim));
        return;
    }

    let del_w = 26.0;
    let date_w = 118.0; // fits "YYYY-MM-DD HH:MM"
    let row_w = screen.width() - 32.0 - del_w - date_w - 12.0;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, entry) in hist.entries().iter().enumerate() {
            let selected = i == hist.selected();
            ui.horizontal(|ui| {
                if ui.add_sized([del_w, 26.0], egui::Button::new("✖")).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
                let row = ui.add_sized(
                    [row_w, 26.0],
                    egui::Button::selectable(
                        selected,
                        egui::RichText::new(&entry.url).color(egui::Color32::WHITE),
                    )
                    .truncate(),
                );
                if row.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(entry.url.clone())));
                }
                ui.add_sized(
                    [date_w, 26.0],
                    egui::Label::new(
                        egui::RichText::new(history::format_time(entry.time)).color(dim),
                    )
                    .truncate(),
                );
            });
        }
    });
}
