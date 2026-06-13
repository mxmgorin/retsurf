//! Rendering of the speed-dial editor overlay (state lives in
//! [`crate::overlay::dial_edit`]): a title, the pinned shortcuts as a deletable
//! tile grid, and a URL field + "Add" button beneath. Navigation (move the
//! selection, add, delete, close) is routed by [`crate::app`]; the mouse can
//! click a tile's ✖, the field, or Add directly. Tiles reuse the start page's
//! [`super::home::paint_tile`] look; deleting and adding go through
//! [`crate::app::MenuAction`].

use super::home::{paint_tile, GAP, GLYPH, TILE_H, TILE_W};
use super::theme::{ACCENT, CLOSE_SIZE};
use crate::app::{AppCommand, MenuAction};
use crate::overlay::dial_edit::{DialEdit, EditItem};
use egui_sdl2::egui;

const BG: egui::Color32 = egui::Color32::from_rgb(0x16, 0x17, 0x1a);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x20, 0x24);
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2d, 0x33);
const INK: egui::Color32 = egui::Color32::from_rgb(0xec, 0xec, 0xea);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x8a, 0x8f, 0x98);

const FIELD_H: f32 = 44.0;

/// Draw the full-screen speed-dial editor over the (blank) start page.
pub(super) fn add_dial_edit(
    ctx: &egui::Context,
    edit: &mut DialEdit,
    pins: &[String],
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("dial_edit"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(BG)
                .inner_margin(egui::Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_min_size(screen.size() - egui::vec2(48.0, 32.0));
                    let field_w = (screen.width() * 0.9).min(620.0);
                    let cols = (((field_w + GAP) / (TILE_W + GAP)).floor() as usize).max(1);
                    edit.set_cols(cols);

                    // Mouse-only ✖ close, pinned to the top-right corner (the
                    // gamepad closes with B). Painted/interacted directly so it
                    // doesn't disturb the centered content flow below.
                    add_close_button(ui, screen, commands);

                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Edit speed dial")
                                .color(INK)
                                .size(18.0)
                                .strong(),
                        );
                        ui.add_space(20.0);
                        add_grid(ui, edit, pins, field_w, cols, commands);
                        ui.add_space(24.0);
                        add_field(ui, edit, field_w);
                        ui.add_space(12.0);
                        add_add_button(ui, edit, field_w, commands);
                    });
                    add_hint_bar(ui, screen);
                });
        });
}

/// The deletable tile grid: one tile per pin (a ✖ badge to delete, the
/// selection ring when focused), centered, with an empty-state line when there
/// are no pins.
fn add_grid(
    ui: &mut egui::Ui,
    edit: &DialEdit,
    pins: &[String],
    width: f32,
    cols: usize,
    commands: &mut Vec<AppCommand>,
) {
    if pins.is_empty() {
        ui.label(egui::RichText::new("No shortcuts yet — add one below.").color(MUTED));
        return;
    }
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            for row_start in (0..pins.len()).step_by(cols) {
                let n = (pins.len() - row_start).min(cols);
                let row_w = n as f32 * TILE_W + (n.saturating_sub(1)) as f32 * GAP;
                ui.allocate_ui_with_layout(
                    egui::vec2(row_w, TILE_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for (offset, url) in pins[row_start..row_start + n].iter().enumerate() {
                            let i = row_start + offset;
                            if add_edit_tile(ui, url, edit.tile() == Some(i), i) {
                                commands.push(AppCommand::Menu(MenuAction::DialRemoveAt(i)));
                            }
                        }
                    },
                );
                ui.add_space(GAP);
            }
        },
    );
}

/// One editor tile: the shared tile visual plus a ✖ delete badge in the
/// top-right. The tile body is inert (edit-only); returns whether the ✖ was
/// clicked.
fn add_edit_tile(ui: &mut egui::Ui, url: &str, selected: bool, index: usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::hover());
    let active = selected || resp.hovered();
    paint_tile(ui.painter(), rect, url, active);

    // ✖ delete badge, top-right of the glyph square.
    let glyph_top = rect.top() + 2.0;
    let badge = egui::Rect::from_min_size(
        egui::pos2(rect.center().x + GLYPH / 2.0 - 16.0, glyph_top),
        egui::vec2(18.0, 18.0),
    );
    let badge_resp = ui.interact(
        badge,
        egui::Id::new(("dial_edit_del", index)),
        egui::Sense::click(),
    );
    let hot = badge_resp.hovered();
    ui.painter()
        .circle_filled(badge.center(), 9.0, if hot { ACCENT } else { SURFACE });
    ui.painter().circle_stroke(
        badge.center(),
        9.0,
        egui::Stroke::new(1.0, if hot { ACCENT } else { BORDER }),
    );
    ui.painter().text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        "✖",
        egui::FontId::proportional(11.0),
        INK,
    );
    badge_resp.clicked()
}

/// The URL entry field: an egui text field (its `dial_edit_url` id keeps egui
/// keyboard focus in sync with the selection); the OSK types into the same
/// buffer on the handheld.
fn add_field(ui: &mut egui::Ui, edit: &mut DialEdit, width: f32) {
    let selected = edit.field_focused();
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
        let edit_widget = egui::TextEdit::singleline(edit.input_mut())
            .id(egui::Id::new("dial_edit_url"))
            .hint_text("Type or paste a URL")
            .frame(egui::Frame::NONE)
            .background_color(egui::Color32::TRANSPARENT)
            .desired_width(f32::INFINITY)
            .font(egui::FontId::proportional(18.0))
            .text_color(INK);
        let resp = ui.add(edit_widget);
        if resp.gained_focus() {
            edit.focus_field();
        }
        // Keep egui keyboard focus tracking the selection (mirrors the start
        // page's search field): hold focus while the field is the selected item,
        // release it when the selection moves to a tile / Add.
        if edit.field_focused() {
            if !resp.has_focus() {
                resp.request_focus();
            }
        } else if resp.has_focus() {
            resp.surrender_focus();
        }
    });
}

/// The full-width "Add" button beneath the field: accent-filled when focused;
/// pins the field's text on click.
fn add_add_button(ui: &mut egui::Ui, edit: &DialEdit, width: f32, commands: &mut Vec<AppCommand>) {
    let focused = edit.item() == EditItem::Add;
    let mut btn = egui::Button::new(
        egui::RichText::new("Add")
            .color(egui::Color32::WHITE)
            .size(15.0),
    )
    .corner_radius(8.0)
    .min_size(egui::vec2(width, FIELD_H - 8.0));
    if focused {
        btn = btn
            .fill(ACCENT.linear_multiply(0.30))
            .stroke(egui::Stroke::new(1.0, ACCENT));
    }
    if ui.add(btn).clicked() {
        commands.push(AppCommand::Menu(MenuAction::DialAdd(
            edit.input().to_string(),
        )));
    }
}

/// The mouse-only ✖ close button in the top-right corner (shared with the menu).
fn add_close_button(ui: &mut egui::Ui, screen: egui::Rect, commands: &mut Vec<AppCommand>) {
    let rect = egui::Rect::from_min_size(
        egui::pos2(screen.right() - 16.0 - CLOSE_SIZE, screen.top() + 14.0),
        egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
    );
    if super::theme::close_button(ui, rect, egui::Id::new("dial_edit_close")).clicked() {
        commands.push(AppCommand::Menu(MenuAction::DialClose));
    }
}

/// A dim one-line control hint pinned near the bottom of the panel.
fn add_hint_bar(ui: &egui::Ui, screen: egui::Rect) {
    ui.painter().text(
        egui::pos2(screen.center().x, screen.bottom() - 22.0),
        egui::Align2::CENTER_CENTER,
        "⏶⏷ select   A add / type   X delete   B back",
        egui::FontId::proportional(12.0),
        MUTED,
    );
}
