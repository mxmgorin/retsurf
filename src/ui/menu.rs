//! Rendering of the full-screen menu overlay (state lives in [`crate::overlay::menu`]):
//! the section bar with the ✖ close and contextual clear actions, and the four
//! section lists (Tabs / Bookmarks / History / Downloads).

use super::theme::{close_button, ACCENT, CLOSE_SIZE, DIM, PANEL_FILL, ROW_FONT};
use crate::app::{AppCommand, MenuAction};
use crate::browser::TabInfo;
use crate::data::history;
use crate::overlay::menu::{Menu, Section};
use egui_sdl2::egui;

/// List rows (tabs / bookmarks / history / downloads) share a height, a corner
/// radius, and a font size so the four sections line up. Rows are taller than
/// egui's default button so they stay legible on a handheld at arm's length,
/// and rounded + vertically spaced (see [`ROW_GAP`]) so the list reads as
/// discrete entries rather than one dense block.
const ROW_H: f32 = 32.0;
const ROW_RADIUS: f32 = 6.0;
const ROW_GAP: f32 = 4.0;

/// Inner padding of the menu panel — the sides get more room than the top and
/// bottom so the lists breathe rather than running to the screen edge. [`SIDES`]
/// is the pair, subtracted from the screen width wherever a row width is derived.
const PAD_X: f32 = 30.0;
const PAD_Y: f32 = 16.0;
const SIDES: f32 = PAD_X * 2.0;
/// The square ✖ delete button leading each row, and the menu's close ✖.
const DEL_W: f32 = 26.0;

/// A row's leading ✖ delete button: dim by default, accent on the selected row
/// so the cursor's delete target is obvious without shouting on every line.
fn delete_button(ui: &mut egui::Ui, selected: bool, dim: egui::Color32) -> egui::Response {
    let color = if selected { ACCENT } else { dim };
    ui.add_sized(
        [DEL_W, ROW_H],
        egui::Button::new(egui::RichText::new("✖").color(color)).corner_radius(ROW_RADIUS),
    )
}

/// A row's ★/☆ bookmark toggle (Tabs / History): a filled ★ in the accent when
/// the URL is bookmarked, a dim hollow ☆ otherwise. Sized like [`delete_button`]
/// so the leading action buttons line up across rows.
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

/// A section's vertical scroll area, capped to the room left down to the screen
/// bottom. The menu panel's `Area` auto-sizes to its content, so an unbounded
/// `ScrollArea` would just grow past the screen and clip instead of scrolling;
/// the explicit `max_height` + `auto_shrink` off make it scroll (and show a bar).
/// The caller pairs this with `scroll_to_me` on the selected row so the
/// gamepad-driven selection stays in view (there's no cursor to drag the bar).
fn section_scroll(ui: &egui::Ui, screen: egui::Rect) -> egui::ScrollArea {
    let max_h = (screen.bottom() - PAD_Y - ui.cursor().top()).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(max_h)
}

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
    let dim = DIM;
    egui::Area::new(egui::Id::new("menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        // Pin the panel at the top-left: with the frame sized to fill the screen
        // exactly, egui must not shift it to "fit" (the shift would cancel the
        // left padding — the bug behind the side margins appearing to do nothing).
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(PANEL_FILL)
                .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
                .show(ui, |ui| {
                    // Size the *content* to the screen minus the frame margins, so
                    // the frame itself fills the screen rather than overflowing it
                    // by 2×PAD (which is what pushed the panel off-edge before).
                    ui.set_min_size(screen.size() - egui::vec2(SIDES, PAD_Y * 2.0));

                    // Mouse-only ✖ close pinned to the top-right corner (the
                    // gamepad closes with B); shared with the dial editor. Painted
                    // independently of the section-bar row so it can't shift it.
                    let close_rect = egui::Rect::from_min_size(
                        egui::pos2(screen.right() - PAD_X - CLOSE_SIZE, screen.top() + PAD_Y),
                        egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
                    );
                    if close_button(ui, close_rect, egui::Id::new("menu_close")).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Close));
                    }

                    // Section bar (active tab highlighted); the active section's
                    // bulk-clear action sits at its right (left of the ✖ corner).
                    ui.horizontal(|ui| {
                        // A little gap between segments turns the flush row of
                        // buttons into a segmented control; the active one wears
                        // egui's accent selection wash (see `theme`).
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for section in Section::ALL {
                            let active = section == menu.section();
                            let tab = egui::Button::selectable(
                                active,
                                egui::RichText::new(section.label())
                                    .color(egui::Color32::WHITE)
                                    .size(ROW_FONT),
                            )
                            .corner_radius(ROW_RADIUS)
                            .min_size(egui::vec2(0.0, 28.0));
                            if ui.add(tab).clicked() {
                                commands.push(AppCommand::Menu(MenuAction::SetSection(section)));
                            }
                        }
                        // Width from `screen`, not `available_width()` (which runs
                        // past the visible edge); reserve the corner ✖'s footprint
                        // so the clear action sits to its left, not under it.
                        let remaining =
                            screen.width() - SIDES - ui.min_rect().width() - (CLOSE_SIZE + 8.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(remaining.max(1.0), 28.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Downloads' bulk-clear lives here (dim, mouse-only).
                                // History's "Clear all" is instead the top row of
                                // its list (see `add_history_section`).
                                let clear_label = match menu.section() {
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
                    // Y is section-specific: Bookmarks pins to the start-page
                    // dial, History/Tabs bookmark the selected entry/tab.
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
                        Section::Bookmarks => {
                            add_bookmarks_section(ui, screen, menu, dim, commands)
                        }
                        Section::History => add_history_section(ui, screen, menu, dim, commands),
                        Section::Downloads => {
                            add_downloads_section(ui, screen, menu, dim, commands)
                        }
                    }
                });
        });
}

/// Tabs section: a leading "+ New tab" action button (selection index 0) over
/// the open tabs (active one marked, selection indices `1..=tabs.len()`). A row
/// opens/switches, its ✖ closes, the button opens a fresh tab.
fn add_tabs_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    menu: &Menu,
    tabs: &[TabInfo],
    selected: usize,
    commands: &mut Vec<AppCommand>,
) {
    let dim = DIM;
    // Width left for the title before the trailing ★ and ✖ buttons (each DEL_W,
    // plus the spacing before each).
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
                // Trailing actions: ★/☆ bookmark (disabled until the tab has a
                // URL), then ✖ close at the far right.
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
            // The row opens (mouse); ✖ at the far right deletes. The gamepad
            // uses the stick + A/X.
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

    let date_w = 118.0; // fits "YYYY-MM-DD HH:MM"
                        // Width left for the URL after the leading ✖ and ★ buttons and the date.
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
                // Trailing actions: ★/☆ bookmark (same as Y), then ✖ delete.
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
