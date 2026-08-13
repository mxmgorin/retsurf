//! Rendering of the full-screen menu overlay (state lives in [`crate::overlay::menu`]):
//! the section bar with the close action, and the four section lists
//! (Tabs / Bookmarks / History / Downloads).

use super::panel::{self, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{self, ACCENT, DIM, ROW_FONT, WARN};
use crate::app::{AppCommand, MenuAction};
use crate::browser::TabInfo;
use crate::data::history;
use crate::overlay::menu::{Menu, Section};
use egui_phosphor::{bold, fill};
use egui_sdl2::egui::{self, AtomExt as _};

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
        egui::Button::new(theme::icon(bold::X).color(color)).corner_radius(ROW_RADIUS),
    )
}

/// A row's bookmark toggle (Tabs / History), sized like [`delete_button`] so the
/// trailing buttons line up across rows.
fn bookmark_button(ui: &mut egui::Ui, bookmarked: bool, dim: egui::Color32) -> egui::Response {
    let star = if bookmarked {
        theme::icon_fill(fill::STAR).color(ACCENT)
    } else {
        theme::icon(bold::STAR).color(dim)
    };
    ui.add_sized(
        [DEL_W, ROW_H],
        egui::Button::new(star).corner_radius(ROW_RADIUS),
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
    row_atoms(
        ui,
        width,
        selected,
        (text.size(ROW_FONT), egui::Atom::grow()),
    )
}

/// [`row_button`] over pre-built atoms (a multi-color label). Mark one atom
/// `shrink` — truncation eats that one; egui otherwise picks the leading run.
fn row_atoms<'a>(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    atoms: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, atoms)
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

/// A URL row's label: site name in white, rest of the URL dim and middle-elided
/// to `width`. Leading with the name makes the list scannable; keeping both ends
/// of the path keeps what differs between two rows of the same site.
fn url_atoms(ui: &egui::Ui, url: &str, pinned: bool, width: f32) -> egui::Atoms<'static> {
    let font = egui::FontId::proportional(ROW_FONT);
    let brand_text = super::home::brand_label(url);
    // Row padding, the gap after the brand, and the pin when there is one.
    let mut budget = width - text_width(ui, &brand_text, &font) - 24.0;
    if pinned {
        budget -= text_width(ui, bold::PUSH_PIN, &font) + 4.0;
    }
    let brand = egui::RichText::new(brand_text)
        .size(ROW_FONT)
        .color(egui::Color32::WHITE);
    let tail = egui::RichText::new(elide_middle(ui, url_tail(url), &font, budget))
        .size(ROW_FONT)
        .color(DIM);
    let mut atoms = egui::Atoms::new((brand, tail.atom_shrink(true), egui::Atom::grow()));
    if pinned {
        atoms.push_left(
            egui::RichText::new(bold::PUSH_PIN)
                .size(ROW_FONT)
                .color(ACCENT),
        );
    }
    atoms
}

/// Rendered width of `text` in `font` (logical px).
fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), DIM)
            .size()
            .x
    })
}

/// Shorten `text` to `budget` px by cutting its middle. egui truncates at the
/// end only, which drops the part of a URL that identifies the page.
fn elide_middle(ui: &egui::Ui, text: &str, font: &egui::FontId, budget: f32) -> String {
    if budget <= 0.0 || text_width(ui, text, font) <= budget {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    // Binary search the number of kept chars: the width is monotonic in it.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if text_width(ui, &join_around_ellipsis(&chars, mid), font) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    join_around_ellipsis(&chars, lo)
}

/// `keep` chars around a central ellipsis: a third leading, the rest trailing.
fn join_around_ellipsis(chars: &[char], keep: usize) -> String {
    let head = keep / 3;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// The leading clear row of History / Downloads. Two presses: the first arms it
/// (the label says so, in the warn color), the second wipes the list. `enabled`
/// dims it further when there is nothing to clear.
fn clear_row(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    selected: bool,
    armed: bool,
    enabled: bool,
    commands: &mut Vec<AppCommand>,
) {
    let text = if armed {
        egui::RichText::new(format!("{label} — press again to confirm")).color(WARN)
    } else if enabled {
        egui::RichText::new(label).color(DIM)
    } else {
        egui::RichText::new(label).color(egui::Color32::from_gray(0x66))
    };
    let resp = row_button(ui, width, selected, text);
    if selected {
        resp.scroll_to_me(Some(egui::Align::Center));
    }
    if resp.clicked() {
        commands.push(AppCommand::Menu(MenuAction::Clear));
    }
}

/// Path, query and fragment of `url`; empty for a bare host.
fn url_tail(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let tail = after_scheme.find('/').map_or("", |i| &after_scheme[i..]);
    if tail == "/" {
        ""
    } else {
        tail
    }
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
            // Both clear actions are the top row of their list, not a bar button:
            // a gamepad can reach a row.
            |_| {},
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
        let (left, right) = (bold::CARET_LEFT, bold::CARET_RIGHT);
        let (up, down) = (bold::CARET_UP, bold::CARET_DOWN);
        ui.label(
            egui::RichText::new(format!(
                "{left}{right} section   {up}{down} select   A open   X delete{y_hint}   B close"
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
        ui.label(
            egui::RichText::new(format!(
                "No bookmarks yet — press {} to add this page.",
                bold::STAR
            ))
            .color(dim),
        );
        return;
    }

    // Fixed widths derived from the screen (not `ui.available_width()`, which is
    // unreliable inside a scroll area and made the list jump horizontally).
    let row_w = screen.width() - SIDES - DEL_W - 6.0; // frame margins + delete + spacing
    section_scroll(ui, screen).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        for (i, url) in bookmarks.urls().iter().enumerate() {
            let selected = i == bookmarks.selected();
            // A leading pin marks a row pinned to the start-page dial (Y toggles).
            ui.horizontal(|ui| {
                let atoms = url_atoms(ui, url, menu.dial.contains(url), row_w);
                let resp = row_atoms(ui, row_w, selected, atoms);
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
        clear_row(
            ui,
            screen.width() - SIDES,
            "Clear finished",
            downloads.selected() == 0,
            menu.clear_armed(),
            downloads.has_finished(),
            commands,
        );
        for (i, item) in downloads.items().iter().enumerate() {
            let selected = downloads.selected() == i + 1; // index 0 is "Clear finished"
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

/// History section: visited URLs (most-recent first) with their visit date, over
/// a leading "Clear all" row.
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
        // Top row (cursor index 0, mirroring Tabs' "+ New tab"): drops every
        // entry, by mouse or A. Dim, to read as a secondary/destructive action
        // set apart from the URL rows.
        clear_row(
            ui,
            screen.width() - SIDES,
            "Clear all",
            hist.selected() == 0,
            menu.clear_armed(),
            true,
            commands,
        );
        for (i, entry) in hist.entries().iter().enumerate() {
            let selected = hist.selected() == i + 1; // index 0 is "Clear all"
            ui.horizontal(|ui| {
                let atoms = url_atoms(ui, &entry.url, false, row_w);
                let resp = row_atoms(ui, row_w, selected, atoms);
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
