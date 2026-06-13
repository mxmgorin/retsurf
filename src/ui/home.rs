//! Rendering of the built-in start page overlay (state lives in
//! [`crate::overlay::home`]): a wordmark, a search / URL field, a speed-dial grid
//! of the pinned shortcuts ([`crate::data::dial`]), and a bottom control-hint
//! bar. Gamepad/keyboard navigation (move the selection, activate a tile) is
//! routed by [`crate::app`]; the mouse can click the field or a tile directly.
//! Tiles open via [`MenuAction::OpenUrl`] (which loads the URL in the active tab)
//! — the same path the menu's lists use.

use super::theme::ACCENT;
use crate::app::{AppCommand, MenuAction};
use crate::data::dial::SETTINGS_PIN;
use crate::overlay::home::Home;
use egui_sdl2::egui;

const BG: egui::Color32 = egui::Color32::from_rgb(0x16, 0x17, 0x1a);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x20, 0x24);
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2d, 0x33);
const INK: egui::Color32 = egui::Color32::from_rgb(0xec, 0xec, 0xea);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x8a, 0x8f, 0x98);

/// Tile footprint (logical px) and grid spacing. Shared with the dial editor.
pub(super) const TILE_W: f32 = 96.0;
pub(super) const TILE_H: f32 = 84.0;
pub(super) const GAP: f32 = 12.0;

/// Draw the start-page overlay over the (blank) web view — below the toolbar
/// (`webview_top`), so the address bar and toolbar buttons stay usable. Any
/// activation is pushed as a command for the app to execute.
pub(super) fn add_home(
    ctx: &egui::Context,
    home: &mut Home,
    pins: &[String],
    webview_top: f32,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let area = egui::Rect::from_min_max(egui::pos2(screen.left(), webview_top), screen.max);
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
                    const WORDMARK_H: f32 = 34.0;
                    const FIELD_H: f32 = 44.0;
                    const GAP_TOP: f32 = 28.0; // wordmark → field
                    const GAP_MID: f32 = 36.0; // field → grid
                                               // One tile per pin plus the trailing "+ Add" tile.
                    let tiles = pins.len() + 1;
                    let rows = tiles.div_ceil(cols);
                    let grid_h = rows as f32 * TILE_H + (rows.saturating_sub(1)) as f32 * GAP;
                    let content_h = WORDMARK_H + GAP_TOP + FIELD_H + GAP_MID + grid_h;
                    let top = ((ui.available_height() - content_h) / 2.0).max(8.0);

                    ui.vertical_centered(|ui| {
                        ui.add_space(top);
                        add_wordmark(ui);
                        ui.add_space(GAP_TOP);
                        add_search(ui, home, field_w);
                        ui.add_space(GAP_MID);
                        add_dial(ui, home, pins, field_w, cols, commands);
                    });
                    add_hint_bar(ui, area);
                });
        });
}

/// The bottom control-hint bar: little key-cap pills with their action, centered
/// near the foot of the page. Painted (not laid out in the centered flow) so it
/// stays pinned to the bottom regardless of how many tiles there are. Plain
/// ASCII letters in pills sidestep egui's gappy gamepad-glyph coverage.
fn add_hint_bar(ui: &egui::Ui, area: egui::Rect) {
    const HINTS: &[(&str, &str)] = &[("A", "Open"), ("☰", "Menu")];
    const PILL_H: f32 = 18.0;
    const PAD: f32 = 6.0; // pill horizontal padding around the key glyph
    const GAP_KL: f32 = 6.0; // key pill → its label
    const GAP_SEG: f32 = 18.0; // between hint segments
    let key_font = egui::FontId::proportional(12.0);
    let label_font = egui::FontId::proportional(12.0);
    let painter = ui.painter();

    // Lay out every glyph first so the row can be centered as a whole.
    let segs: Vec<_> = HINTS
        .iter()
        .map(|(key, label)| {
            let kg = painter.layout_no_wrap(key.to_string(), key_font.clone(), INK);
            let lg = painter.layout_no_wrap(label.to_string(), label_font.clone(), MUTED);
            let pill_w = kg.size().x + PAD * 2.0;
            let seg_w = pill_w + GAP_KL + lg.size().x;
            (kg, lg, pill_w, seg_w)
        })
        .collect();
    let total: f32 = segs.iter().map(|s| s.3).sum::<f32>() + GAP_SEG * (segs.len() - 1) as f32;

    let cy = area.bottom() - 18.0;
    let mut x = area.center().x - total / 2.0;
    for (kg, lg, pill_w, seg_w) in segs {
        let pill =
            egui::Rect::from_min_size(egui::pos2(x, cy - PILL_H / 2.0), egui::vec2(pill_w, PILL_H));
        painter.rect_filled(pill, 5.0, SURFACE);
        painter.rect_stroke(
            pill,
            5.0,
            egui::Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
        painter.galley(pill.center() - kg.size() / 2.0, kg, INK);
        painter.galley(
            egui::pos2(x + pill_w + GAP_KL, cy - lg.size().y / 2.0),
            lg,
            MUTED,
        );
        x += seg_w + GAP_SEG;
    }
}

/// The brand wordmark: a large two-tone "RET·SURF" logotype — the "RET" in ink,
/// "SURF" in the accent — so the start page reads as branded rather than blank.
/// Built as one `LayoutJob` so both colors stay on a single centered line, with
/// wide letter-spacing for a logo feel.
fn add_wordmark(ui: &mut egui::Ui) {
    const SIZE: f32 = 30.0;
    let mut job = egui::text::LayoutJob::default();
    let fmt = |color: egui::Color32| egui::TextFormat {
        font_id: egui::FontId::proportional(SIZE),
        color,
        extra_letter_spacing: 3.0,
        ..Default::default()
    };
    job.append("RET", 0.0, fmt(INK));
    job.append("SURF", 0.0, fmt(ACCENT));
    ui.label(job);
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

/// The speed-dial grid: one tile per pinned shortcut (the brand initial over its
/// name), followed by a trailing "Edit" tile that opens the speed-dial editor.
fn add_dial(
    ui: &mut egui::Ui,
    home: &Home,
    pins: &[String],
    width: f32,
    cols: usize,
    commands: &mut Vec<AppCommand>,
) {
    let tiles = pins.len() + 1; // + the trailing "Edit" tile
                                // Center the grid within the field width.
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            for row_start in (0..tiles).step_by(cols) {
                // Allocate each row at its exact content width so a partial last
                // row (or a grid narrower than `cols`) stays centred — a plain
                // `ui.horizontal` would take the full width and left-align.
                let n = (tiles - row_start).min(cols);
                let row_w = n as f32 * TILE_W + (n.saturating_sub(1)) as f32 * GAP;
                ui.allocate_ui_with_layout(
                    egui::vec2(row_w, TILE_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for i in row_start..row_start + n {
                            let selected = home.tile() == Some(i);
                            match pins.get(i) {
                                Some(url) => {
                                    if add_tile(ui, url, selected).clicked() {
                                        commands.push(AppCommand::Menu(MenuAction::OpenUrl(
                                            url.clone(),
                                        )));
                                    }
                                }
                                // i == pins.len(): the trailing "Edit" tile.
                                None => {
                                    if add_edit_tile(ui, selected).clicked() {
                                        commands.push(AppCommand::Menu(MenuAction::DialEdit));
                                    }
                                }
                            }
                        }
                    },
                );
                ui.add_space(GAP);
            }
        },
    );
}

/// Glyph-square side length within a tile.
pub(super) const GLYPH: f32 = 52.0;

/// One speed-dial tile: a rounded "glyph" square holding the brand initial, with
/// the brand name beneath it — accent-ringed and brightened when selected or
/// hovered. Custom-painted (not a Button) for the two-tier look. Returns its
/// click response.
fn add_tile(ui: &mut egui::Ui, url: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::click());
    paint_tile(ui.painter(), rect, url, selected || resp.hovered());
    resp
}

/// Paint a speed-dial tile's visuals (glyph square + brand initial + name) into
/// `rect`. Shared by the start page and the dial editor ([`super::dial_edit`]);
/// the caller owns the click region (and any extra overlays like a ✖ badge).
pub(super) fn paint_tile(painter: &egui::Painter, rect: egui::Rect, url: &str, active: bool) {
    // Glyph square, centered near the top of the tile.
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + GLYPH / 2.0 + 2.0),
        egui::vec2(GLYPH, GLYPH),
    );
    painter.rect_filled(glyph, 12.0, SURFACE);
    painter.rect_stroke(
        glyph,
        12.0,
        egui::Stroke::new(
            if active { 2.0 } else { 1.0 },
            if active { ACCENT } else { BORDER },
        ),
        egui::StrokeKind::Inside,
    );

    // The settings sentinel isn't a real address: show a ⚙ glyph and "Settings"
    // rather than the garbage `brand_label` would derive from `retsurf:settings`.
    let (glyph_text, name) = if url == SETTINGS_PIN {
        ("⚙".to_string(), "Settings".to_string())
    } else {
        let label = brand_label(url);
        let initial = label
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        (initial, label)
    };
    painter.text(
        glyph.center(),
        egui::Align2::CENTER_CENTER,
        glyph_text,
        egui::FontId::proportional(22.0),
        INK,
    );

    // Brand name under the glyph (truncated so a long name can't overflow).
    painter.text(
        egui::pos2(rect.center().x, glyph.bottom() + 14.0),
        egui::Align2::CENTER_CENTER,
        truncate(&name, 12),
        egui::FontId::proportional(12.0),
        if active { INK } else { MUTED },
    );
}

/// The trailing "Edit" tile: an empty (fill-less) glyph square holding a ✏, with
/// "Edit" beneath — accent-ringed and brightened when selected or hovered, like a
/// real tile but unfilled so it reads as an action slot. Opens the dial editor.
fn add_edit_tile(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::click());
    let active = selected || resp.hovered();
    let painter = ui.painter();

    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + GLYPH / 2.0 + 2.0),
        egui::vec2(GLYPH, GLYPH),
    );
    painter.rect_stroke(
        glyph,
        12.0,
        egui::Stroke::new(
            if active { 2.0 } else { 1.0 },
            if active { ACCENT } else { BORDER },
        ),
        egui::StrokeKind::Inside,
    );
    painter.text(
        glyph.center(),
        egui::Align2::CENTER_CENTER,
        "✏",
        egui::FontId::proportional(24.0),
        if active { ACCENT } else { MUTED },
    );
    painter.text(
        egui::pos2(rect.center().x, glyph.bottom() + 14.0),
        egui::Align2::CENTER_CENTER,
        "Edit",
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
