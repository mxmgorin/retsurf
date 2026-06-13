//! Rendering of the built-in start page overlay (state lives in
//! [`crate::overlay::home`]): a wordmark, a search / URL field, and a speed-dial
//! grid of the saved bookmarks. Gamepad/keyboard navigation (move the selection,
//! activate) is routed by [`crate::app`]; the mouse can click the field or a
//! tile directly. Tiles open via [`MenuAction::OpenUrl`] (which loads the URL in
//! the active tab) — the same path the menu's lists use.

use crate::app::{AppCommand, MenuAction};
use crate::overlay::home::Home;
use egui_sdl2::egui;

const BG: egui::Color32 = egui::Color32::from_rgb(0x16, 0x17, 0x1a);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x20, 0x24);
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2d, 0x33);
const INK: egui::Color32 = egui::Color32::from_rgb(0xec, 0xec, 0xea);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x8a, 0x8f, 0x98);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3f, 0xb8, 0xa0);

/// Tile footprint (logical px) and grid spacing.
const TILE_W: f32 = 96.0;
const TILE_H: f32 = 84.0;
const GAP: f32 = 12.0;

/// Draw the start-page overlay over the (blank) web view — below the toolbar
/// (`webview_top`), so the address bar and toolbar buttons stay usable. Any
/// activation is pushed as a command for the app to execute.
pub(super) fn add_home(
    ctx: &egui::Context,
    home: &mut Home,
    bookmarks: &[String],
    webview_top: f32,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let area = egui::Rect::from_min_max(
        egui::pos2(screen.left(), webview_top),
        screen.max,
    );
    egui::Area::new(egui::Id::new("home"))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        // Don't let egui shift the area up to fit the screen — it must stay
        // pinned below the toolbar (`webview_top`), even if content overflows.
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(BG)
                .inner_margin(0.0)
                .show(ui, |ui| {
                    ui.set_min_size(area.size());
                    // Field/grid width: the artifact's min(620, 90%).
                    let field_w = (area.width() * 0.9).min(620.0);
                    let cols = (((field_w + GAP) / (TILE_W + GAP)).floor() as usize).max(1);
                    home.set_cols(cols);

                    // Vertically center the whole block (wordmark + field + grid),
                    // like the original page did, by padding the top by half the
                    // slack. Heights below are the laid-out sizes plus the gaps.
                    const WORDMARK_H: f32 = 18.0;
                    const FIELD_H: f32 = 44.0;
                    const GAP_TOP: f32 = 28.0; // wordmark → field
                    const GAP_MID: f32 = 36.0; // field → grid
                    let rows = if bookmarks.is_empty() {
                        1
                    } else {
                        bookmarks.len().div_ceil(cols)
                    };
                    let grid_h = if bookmarks.is_empty() {
                        20.0
                    } else {
                        rows as f32 * TILE_H + (rows.saturating_sub(1)) as f32 * GAP
                    };
                    let content_h = WORDMARK_H + GAP_TOP + FIELD_H + GAP_MID + grid_h;
                    let top = ((ui.available_height() - content_h) / 2.0).max(8.0);

                    ui.vertical_centered(|ui| {
                        ui.add_space(top);
                        ui.label(
                            egui::RichText::new("RETSURF")
                                .color(MUTED)
                                .size(14.0)
                                .strong(),
                        );
                        ui.add_space(GAP_TOP);
                        add_search(ui, home, field_w);
                        ui.add_space(GAP_MID);
                        add_dial(ui, home, bookmarks, field_w, cols, commands);
                    });
                });
        });
}

/// The hero search / URL field. Editable directly (desktop keyboard); on the
/// handheld the OSK writes into the same buffer. Enter submits it (handled in
/// the keyboard/router layer).
fn add_search(ui: &mut egui::Ui, home: &mut Home, width: f32) {
    let selected = home.search_focused();
    let frame = egui::Frame::default()
        .fill(SURFACE)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .corner_radius(10.0)
        .stroke(egui::Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { ACCENT } else { BORDER },
        ));
    frame.show(ui, |ui| {
        ui.set_width(width);
        let edit = egui::TextEdit::singleline(home.input_mut())
            .id(egui::Id::new("home_search"))
            .hint_text("Search or enter address")
            .frame(egui::Frame::NONE)
            .background_color(egui::Color32::TRANSPARENT)
            .desired_width(f32::INFINITY)
            .font(egui::FontId::proportional(20.0))
            .text_color(INK);
        let resp = ui.add(edit);
        // Clicking the field selects it (so the highlight follows the mouse).
        if resp.gained_focus() {
            home.focus_search();
        }
        // Keep egui keyboard focus in sync with the selection: focus the field
        // when it's the selected item (so a keyboard can type immediately) and
        // release it when the selection moves to a tile (so arrows navigate).
        // Enter is handled in the keyboard/router layer, not via egui's
        // lost-focus (which this per-frame re-focus would race).
        if home.search_focused() {
            if !resp.has_focus() {
                resp.request_focus();
            }
        } else if resp.has_focus() {
            resp.surrender_focus();
        }
    });
}

/// The speed-dial grid: one tile per bookmark, the brand initial over its name.
fn add_dial(
    ui: &mut egui::Ui,
    home: &Home,
    bookmarks: &[String],
    width: f32,
    cols: usize,
    commands: &mut Vec<AppCommand>,
) {
    if bookmarks.is_empty() {
        ui.label(
            egui::RichText::new("No bookmarks yet — press ★ to add a page.").color(MUTED),
        );
        return;
    }
    // Center the grid within the field width.
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            for (r, row) in bookmarks.chunks(cols).enumerate() {
                ui.horizontal(|ui| {
                    for (j, url) in row.iter().enumerate() {
                        let i = r * cols + j;
                        if add_tile(ui, url, home.tile() == Some(i)).clicked() {
                            commands.push(AppCommand::Menu(MenuAction::OpenUrl(url.clone())));
                        }
                    }
                });
                ui.add_space(GAP);
            }
        },
    );
}

/// Glyph-square side length within a tile.
const GLYPH: f32 = 52.0;

/// One speed-dial tile: a rounded "glyph" square holding the brand initial, with
/// the brand name beneath it — accent-ringed and brightened when selected or
/// hovered. Custom-painted (not a Button) for the two-tier look. Returns its
/// click response.
fn add_tile(ui: &mut egui::Ui, url: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::click());
    let active = selected || resp.hovered();
    let painter = ui.painter();

    // Glyph square, centered near the top of the tile.
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + GLYPH / 2.0 + 2.0),
        egui::vec2(GLYPH, GLYPH),
    );
    painter.rect_filled(glyph, 12.0, SURFACE);
    painter.rect_stroke(
        glyph,
        12.0,
        egui::Stroke::new(if active { 2.0 } else { 1.0 }, if active { ACCENT } else { BORDER }),
        egui::StrokeKind::Inside,
    );

    let label = brand_label(url);
    let initial = label
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    painter.text(
        glyph.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(22.0),
        INK,
    );

    // Brand name under the glyph (truncated so a long name can't overflow).
    painter.text(
        egui::pos2(rect.center().x, glyph.bottom() + 14.0),
        egui::Align2::CENTER_CENTER,
        truncate(&label, 12),
        egui::FontId::proportional(12.0),
        if active { INK } else { MUTED },
    );
    resp
}

/// Trim a label to `max` characters, appending `…` when shortened.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

/// A short label for a tile: the registrable domain name (`duckduckgo.com` →
/// `duckduckgo`, `en.wikipedia.org` → `wikipedia`, `bbc.co.uk` → `bbc`), falling
/// back to the host, then the raw string.
fn brand_label(url: &str) -> String {
    let Some(host) = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    else {
        return url.to_string();
    };
    let host = host.trim_start_matches("www.");
    let parts: Vec<&str> = host.split('.').filter(|s| !s.is_empty()).collect();
    let n = parts.len();
    if n <= 1 {
        return host.to_string();
    }
    let suffix_len = if n >= 3 && parts[n - 2].len() <= 3 && parts[n - 1].len() == 2 {
        2
    } else {
        1
    };
    parts[n - suffix_len - 1].to_string()
}
