//! Rendering of the on-screen keyboard (state and input handling live in
//! [`crate::overlay::osk`]).

use super::theme::{ACCENT, SCRIM};
use crate::overlay::osk::{Key, Osk};
use egui_sdl2::egui;

const KEY_W: f32 = 36.0;
const KEY_H: f32 = 38.0;
const KEY_GAP: f32 = 4.0;

/// Tab, Caps, Backspace, Lang, Clear, Hide — and the cap on a named key.
const WIDE_W: f32 = 54.0;

/// What every row spans: the top row's thirteen characters and its Backspace.
/// The other rows are sized by hand to match, and a short one centres in it.
const ROW_SPAN: f32 = 13.0 * KEY_W + WIDE_W + 13.0 * KEY_GAP;

/// The picker's caption, and the air under it.
const CAPTION_FONT: f32 = 18.0;
const CAPTION_GAP: f32 = 28.0;

/// The keyboard's own layer, which the dimming and the caption share with it so
/// they need no ordering of their own.
fn osk_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("osk"))
}

/// The gamepad button that directly triggers a key, shown as a corner badge so
/// the shortcuts are discoverable. Mirrors the bindings in [`crate::overlay::osk`]
/// (and the router); keys without a dedicated button use D-pad + **A**.
fn button_hint(key: &Key) -> Option<&'static str> {
    match key {
        Key::Backspace => Some("X"),
        Key::Space => Some("Y"),
        Key::Shift => Some("L2"),
        Key::Enter => Some("R2"),
        Key::Hide => Some("B"),
        _ => None,
    }
}

/// An even share of the span, capped so a row of three is not three slabs.
fn named_width(row_len: usize) -> f32 {
    let share = (ROW_SPAN - KEY_GAP * row_len.saturating_sub(1) as f32) / row_len as f32;
    share.min(WIDE_W)
}

/// How wide one row lays out, gaps included.
fn row_width(row: &[Key], key_width: &impl Fn(usize, &Key) -> f32) -> f32 {
    let keys: f32 = row.iter().map(|key| key_width(row.len(), key)).sum();
    keys + KEY_GAP * row.len().saturating_sub(1) as f32
}

/// What the frame ends up sized to, and so what a shorter row centres in.
fn grid_width(osk: &Osk, key_width: &impl Fn(usize, &Key) -> f32) -> f32 {
    osk.keys()
        .iter()
        .map(|row| row_width(row, key_width))
        .fold(0.0, f32::max)
}

/// Draw the on-screen keyboard, Steam-Deck style: a dark rounded overlay anchored
/// to the bottom, with the selected key (and active Shift/Caps) highlighted.
///
/// `bottom_inset` lifts the keyboard off the bottom edge by that many points —
/// used to clear a bottom toolbar so the address bar being typed into stays
/// visible just below the keys (0 for a top toolbar).
///
/// Returns the drawn height (logical px) — what the page has to scroll past to
/// keep a field it owns visible.
pub(super) fn add_osk(ctx: &egui::Context, osk: &Osk, bottom_inset: f32) -> f32 {
    let selected = osk.selected();
    let shift = osk.shift();
    let highlight = ACCENT;
    let key_fill = egui::Color32::from_rgb(0x3a, 0x3a, 0x40);
    let hint = egui::Color32::from_gray(150);
    // Hand-tuned to fill `ROW_SPAN`, each on its own shorter row; Space gives up
    // the 40 the Fn key took.
    let key_width = |row_len: usize, key: &Key| match key {
        Key::Space => 200.0,
        Key::Shift => 85.0,
        Key::Enter => 76.0,
        Key::Tab | Key::Caps | Key::Backspace | Key::Lang | Key::Clear | Key::Hide => WIDE_W,
        Key::Named { .. } => named_width(row_len),
        _ => KEY_W,
    };

    // A picker has to read as a question, not as a keyboard that happens to be up.
    if osk.picking() {
        ctx.layer_painter(osk_layer())
            .rect_filled(ctx.content_rect(), 0.0, SCRIM);
    }

    let area = egui::Area::new(egui::Id::new("osk"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -bottom_inset))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_rgba_unmultiplied(0x18, 0x18, 0x1c, 245))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(KEY_GAP, 5.0);
                    // The area sizes to the keys, so egui has no width to centre
                    // against until they are laid out.
                    let width = grid_width(osk, &key_width);
                    for (r, row) in osk.keys().iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_space((width - row_width(row, &key_width)) / 2.0);
                            for (c, key) in row.iter().enumerate() {
                                let is_sel = (r, c) == selected;
                                let active = is_sel
                                    || (*key == Key::Shift && shift)
                                    || (*key == Key::Caps && osk.caps);
                                let size = egui::vec2(key_width(row.len(), key), KEY_H);
                                let fill = if active { highlight } else { key_fill };
                                let button = egui::Button::new(
                                    egui::RichText::new(osk.key_label(*key))
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(fill)
                                .corner_radius(6.0)
                                .min_size(size);
                                let response = ui.add(button);
                                // Physical-keyboard style: the shifted symbol sits
                                // small in the corner, and the two swap under Shift.
                                if let Key::Char(ch) = key {
                                    let main = osk.layout().resolve_char(*ch, shift, osk.caps);
                                    let alt = osk.layout().resolve_char(*ch, !shift, osk.caps);
                                    if !ch.is_alphabetic() && alt != main {
                                        ui.painter().text(
                                            response.rect.right_top() + egui::vec2(-4.0, 2.0),
                                            egui::Align2::RIGHT_TOP,
                                            alt,
                                            egui::FontId::proportional(10.0),
                                            hint,
                                        );
                                    }
                                }
                                // The keys with a direct gamepad shortcut wear it
                                // as a small badge in the top-left corner.
                                if let Some(btn) = button_hint(key) {
                                    ui.painter().text(
                                        response.rect.left_top() + egui::vec2(4.0, 2.0),
                                        egui::Align2::LEFT_TOP,
                                        btn,
                                        egui::FontId::proportional(10.0),
                                        hint,
                                    );
                                }
                            }
                        });
                    }
                });
        });
    // Over the dimming, not inside the frame, where it read as a row of keys.
    if osk.picking() {
        let keys = area.response.rect;
        ctx.layer_painter(osk_layer()).text(
            egui::pos2(keys.center().x, keys.top() - CAPTION_GAP),
            egui::Align2::CENTER_BOTTOM,
            "Pick a key",
            egui::FontId::proportional(CAPTION_FONT),
            ACCENT,
        );
    }
    area.response.rect.height()
}
