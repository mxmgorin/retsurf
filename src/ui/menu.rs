//! Rendering of the full-screen menu overlay (state lives in [`crate::overlay::menu`]):
//! the section bar with the close and contextual clear actions, and the four
//! section lists (Tabs / Bookmarks / History / Downloads).

use super::panel::{self, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{ACCENT, DIM, ROW_FONT};
use crate::app::{AppCommand, MenuAction};
use crate::browser::TabInfo;
use crate::data::history;
use crate::overlay::menu::{Menu, Section};
use egui_sdl2::egui;

/// Shared list-row height — taller than egui's default so rows stay legible on a
/// handheld; radius, gap, and padding come from [`super::panel`].
const ROW_H: f32 = 32.0;
/// The square delete button leading each row.
const DEL_W: f32 = 26.0;

/// A row's leading delete button, accent on the selected row.
fn delete_button(ui: &mut egui::Ui, selected: bool, dim: egui::Color32) -> egui::Response {
    let color = if selected { ACCENT } else { dim };
    ui.add_sized(
        [DEL_W, ROW_H],
        egui::Button::new(egui::RichText::new("✖").color(color)).corner_radius(ROW_RADIUS),
    )
}

/// A row's bookmark toggle (Tabs / History), sized like [`delete_button`] so the
/// trailing buttons line up across rows.
fn bookmark_button(ui: &mut egui::Ui, bookmarked: bool, dim: egui::Color32) -> egui::Response {
    let (glyph, color) = if bookmarked {
        ("★", ACCENT)
    } else {
        ("☆", dim)
    };
    ui.add_sized(
        [DEL_W, ROW_H],
        egui::Button::new(egui::RichText::new(glyph).color(color)).corner_radius(ROW_RADIUS),
    )
}

/// A selectable list row at the standard height: rounded, truncated, the shared
/// font size, with its label left-aligned. The caller supplies the colored
/// label; the trailing [`egui::Atom::grow`] fills the rest of the row so the
/// text sits at the left edge instead of egui's default centering.
fn row_button(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    text: egui::RichText,
) -> egui::Response {
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (text.size(ROW_FONT), egui::Atom::grow()))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

/// Draw the menu overlay: the section bar over the active section's list, plus a
/// one-line control hint. Left/Right switch section, Up/Down move, A open,
/// X delete, B close.
pub(super) fn add_menu(
    ctx: &egui::Context,
    menu: &Menu,
    tabs: &[TabInfo],
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let dim = DIM;
    let closed = panel::panel(ctx, "menu", screen, |ui| {
        let clicked = panel::section_bar(
            ui,
            screen,
            Section::ALL,
            menu.section(),
            Section::label,
            |ui| {
                // Downloads clears from the bar; History's "Clear all" is the top
                // row of its list instead (see `add_history_section`).
                if menu.section() == Section::Downloads && menu.downloads.has_finished() {
                    let clear = egui::Button::new(egui::RichText::new("Clear finished").color(dim));
                    if ui.add(clear).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Clear));
                    }
                }
            },
        );
        if let Some(section) = clicked {
            commands.push(AppCommand::Menu(MenuAction::SetSection(section)));
        }
        // Y is section-specific: Bookmarks pins to the dial, History/Tabs bookmark.
        let y_hint = match menu.section() {
            Section::Bookmarks => "   Y pin",
            Section::History | Section::Tabs => "   Y bookmark",
            Section::Downloads => "",
        };
        ui.label(
            egui::RichText::new(format!(
                "⏴⏵ section   ⏶⏷ select   A open   X delete{y_hint}   B close"
            ))
            .color(dim),
        );
        ui.add_space(8.0);

        match menu.section() {
            Section::Tabs => {
                add_tabs_section(ui, screen, menu, tabs, menu.tab_selected(), commands)
            }
            Section::Bookmarks => add_bookmarks_section(ui, screen, menu, dim, commands),
            Section::History => add_history_section(ui, screen, menu, dim, commands),
            Section::Downloads => add_downloads_section(ui, screen, menu, dim, commands),
        }
    });
    if closed {
        commands.push(AppCommand::Menu(MenuAction::Close));
    }
}

/// Tabs section: a leading "+ New tab" row (selection index 0) over the open tabs
/// (indices `1..=tabs.len()`), each with a bookmark toggle and a close button.
fn add_tabs_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    menu: &Menu,
    tabs: &[TabInfo],
    selected: usize,
    commands: &mut Vec<AppCommand>,
) {
    let dim = DIM;
    // Title width, less the two trailing buttons and the spacing before each.
    let row_w = screen.width() - SIDES - 2.0 * DEL_W - 12.0;
    section_scroll(ui, screen).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        // "+ New tab" action at the top (selection index 0): a plain full-width
        // row, same height/indent as the tab rows below, marked by the selectable
        // highlight when it's the cursor row (no fill of its own).
        let new_tab = row_button(
            ui,
            screen.width() - SIDES,
            selected == 0,
            egui::RichText::new("+ New tab").color(egui::Color32::WHITE),
        );
        if selected == 0 {
            new_tab.scroll_to_me(Some(egui::Align::Center));
        }
        if new_tab.clicked() {
            commands.push(AppCommand::Menu(MenuAction::NewTab));
        }

        for (i, tab) in tabs.iter().enumerate() {
            let sel = selected == i + 1; // index 0 is the "+ New tab" button
            ui.horizontal(|ui| {
                // The active (shown) tab stands out in the accent color and bold;
                // the cursor's row uses the selectable highlight, so the two are
                // distinguishable even on the same row.
                let text = if tab.active {
                    egui::RichText::new(&tab.title).color(ACCENT).strong()
                } else {
                    egui::RichText::new(&tab.title).color(egui::Color32::WHITE)
                };
                let resp = row_button(ui, row_w, sel, text);
                if sel {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenTab(i)));
                }
                // The bookmark toggle needs a URL; close sits at the far right.
                let can_bookmark = !tab.url.is_empty();
                let marked = can_bookmark && menu.is_bookmarked(&tab.url);
                if bookmark_button(ui, marked, dim).clicked() && can_bookmark {
                    commands.push(AppCommand::Menu(MenuAction::ToggleBookmark(
                        tab.url.clone(),
                    )));
                }
                if delete_button(ui, sel, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::CloseTab(i)));
                }
            });
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
        ui.label(egui::RichText::new("No bookmarks yet — press ★ to add this page.").color(dim));
        return;
    }

    // Fixed widths derived from the screen (not `ui.available_width()`, which is
    // unreliable inside a scroll area and made the list jump horizontally).
    let row_w = screen.width() - SIDES - DEL_W - 6.0; // frame margins + delete + spacing
    section_scroll(ui, screen).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        for (i, url) in bookmarks.urls().iter().enumerate() {
            let selected = i == bookmarks.selected();
            // A leading 📌 marks a row pinned to the start-page dial; Y toggles
            // the pin (see the legend).
            let label = if menu.dial.contains(url) {
                format!("📌 {url}")
            } else {
                url.clone()
            };
            ui.horizontal(|ui| {
                let text = egui::RichText::new(label).color(egui::Color32::WHITE);
                let resp = row_button(ui, row_w, selected, text);
                if selected {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(url.clone())));
                }
                if delete_button(ui, selected, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
            });
        }
    });
}

/// Downloads section: most-recent first, each row showing the file name and a
/// status (progress, size + date, or the error). Delete cancels or removes the
/// entry (the file on disk is kept); a finished row opens the file.
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

    let status_w = 170.0; // fits "100% · 999.9 MB / 999.9 MB"-ish, truncated past that
    let row_w = screen.width() - SIDES - DEL_W - status_w - 12.0;
    section_scroll(ui, screen).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        for (i, item) in downloads.items().iter().enumerate() {
            let selected = i == downloads.selected();
            ui.horizontal(|ui| {
                let resp = row_button(
                    ui,
                    row_w,
                    selected,
                    egui::RichText::new(&item.filename).color(egui::Color32::WHITE),
                );
                if selected {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    if let Some(url) = downloads.open_url(i) {
                        commands.push(AppCommand::Menu(MenuAction::OpenUrl(url)));
                    }
                }
                ui.add_sized(
                    [status_w, ROW_H],
                    egui::Label::new(egui::RichText::new(item.status_text()).color(dim)).truncate(),
                );
                if delete_button(ui, selected, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
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

    // `date_w` fits "YYYY-MM-DD HH:MM"; the URL gets what's left after the date,
    // the two trailing buttons, and their spacing.
    let date_w = 118.0;
    let row_w = screen.width() - SIDES - 2.0 * DEL_W - date_w - 18.0;
    section_scroll(ui, screen).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        // "Clear all" as the top row (cursor index 0, mirroring Tabs' "+ New
        // tab"): drops every entry, by mouse or A. Dim, to read as a secondary/
        // destructive action set apart from the URL rows.
        let clear = row_button(
            ui,
            screen.width() - SIDES,
            hist.selected() == 0,
            egui::RichText::new("Clear all").color(dim),
        );
        if hist.selected() == 0 {
            clear.scroll_to_me(Some(egui::Align::Center));
        }
        if clear.clicked() {
            commands.push(AppCommand::Menu(MenuAction::Clear));
        }
        for (i, entry) in hist.entries().iter().enumerate() {
            let selected = hist.selected() == i + 1; // index 0 is "Clear all"
            ui.horizontal(|ui| {
                let text = egui::RichText::new(&entry.url).color(egui::Color32::WHITE);
                let resp = row_button(ui, row_w, selected, text);
                if selected {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(entry.url.clone())));
                }
                ui.add_sized(
                    [date_w, ROW_H],
                    egui::Label::new(
                        egui::RichText::new(history::format_time(entry.time)).color(dim),
                    )
                    .truncate(),
                );
                // Trailing actions: the bookmark toggle (same as Y), then delete.
                let marked = menu.is_bookmarked(&entry.url);
                if bookmark_button(ui, marked, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::ToggleBookmark(
                        entry.url.clone(),
                    )));
                }
                if delete_button(ui, selected, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
            });
        }
    });
}
