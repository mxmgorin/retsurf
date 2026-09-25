//! Rendering of the on-screen keyboard (state and input handling live in
//! [`crate::overlay::osk`]).

use super::theme::{ACCENT, SCRIM};
use crate::config::PadLayout;
use crate::overlay::osk::{Face, FaceLabels, Key, Osk, WHEEL_SECTORS};
use egui_phosphor::bold;
use egui_sdl2::egui;
use std::f32::consts::TAU;

const KEY_W: f32 = 36.0;
const KEY_H: f32 = 38.0;
const KEY_GAP: f32 = 4.0;

/// Tab, Caps, Backspace, Lang, Clear, Hide — and the cap on a named key.
const WIDE_W: f32 = 54.0;

/// What every row spans: the top row's thirteen characters and its Backspace.
/// The other rows are sized by hand to match, and a short one centres in it.
const ROW_SPAN: f32 = 13.0 * KEY_W + WIDE_W + 13.0 * KEY_GAP;

const KEY_FILL: egui::Color32 = egui::Color32::from_rgb(0x3a, 0x3a, 0x40);
const HINT: egui::Color32 = egui::Color32::from_gray(150);

/// The wheel's geometry: group centres sit on `WHEEL_RADIUS`, each group's
/// characters on a `FACE_RADIUS` button `FACE_OFFSET` out toward their face.
const WHEEL_RADIUS: f32 = 92.0;
const GROUP_RADIUS: f32 = 34.0;
const FACE_OFFSET: f32 = 16.0;
const FACE_RADIUS: f32 = 9.5;
/// A face button outside the aimed group: grey, so only the aimed one reads.
const FACE_IDLE: egui::Color32 = egui::Color32::from_gray(0x5a);
const HUB_RADIUS: f32 = 30.0;
const GROUP_FONT: f32 = 13.0;
const AIMED_FONT: f32 = 15.0;
const LEGEND_FONT: f32 = 11.0;
const HUB_ICON: f32 = 14.0;
const HUB_FONT: f32 = 10.0;
/// The drawn space mark, a bracket open upward.
const SPACE_MARK_W: f32 = 12.0;
const SPACE_MARK_H: f32 = 4.0;
const SPACE_MARK_STROKE: f32 = 1.5;
const AIMED_FILL: egui::Color32 = egui::Color32::from_rgb(0x4a, 0x4a, 0x52);
const AIMED_RING: f32 = 2.5;

/// The picker's caption, and the air under it.
const CAPTION_FONT: f32 = 18.0;
const CAPTION_GAP: f32 = 28.0;

/// The keyboard's own layer, which the dimming and the caption share with it so
/// they need no ordering of their own.
fn osk_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("osk"))
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

/// The dark rounded panel both styles sit on.
fn panel() -> egui::Frame {
    egui::Frame::default()
        .fill(egui::Color32::from_rgba_unmultiplied(0x18, 0x18, 0x1c, 245))
        .corner_radius(12.0)
        .inner_margin(12.0)
}

/// The keyboard's area, anchored to the bottom and lifted by `bottom_inset`.
fn osk_area(bottom_inset: f32) -> egui::Area {
    egui::Area::new(egui::Id::new("osk"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -bottom_inset))
}

/// Draw the on-screen keyboard: a dark rounded overlay anchored to the bottom.
/// `bottom_inset` lifts it off that edge, to clear a bottom toolbar. Returns the
/// drawn height (logical px), which the page scrolls a field of its own past.
pub(super) fn add_osk(ctx: &egui::Context, osk: &Osk, layout: PadLayout, bottom_inset: f32) -> f32 {
    if osk.wheel() {
        return add_wheel(ctx, osk, layout, bottom_inset);
    }
    let face = FaceLabels::of(layout);
    let selected = osk.selected();
    let shift = osk.shift();
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

    let area = osk_area(bottom_inset).show(ctx, |ui| {
        panel().show(ui, |ui| {
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
                        let fill = if active { ACCENT } else { KEY_FILL };
                        let button = egui::Button::new(
                            egui::RichText::new(osk.key_label(*key)).color(egui::Color32::WHITE),
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
                                    HINT,
                                );
                            }
                        }
                        // The keys with a direct gamepad shortcut wear it
                        // as a small badge in the top-left corner.
                        if let Some(btn) = key.button_hint(face) {
                            ui.painter().text(
                                response.rect.left_top() + egui::vec2(4.0, 2.0),
                                egui::Align2::LEFT_TOP,
                                btn,
                                egui::FontId::proportional(10.0),
                                HINT,
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

/// Which way from a group's centre `face`'s character sits.
fn face_dir(face: Face) -> egui::Vec2 {
    match face {
        Face::North => egui::vec2(0.0, -1.0),
        Face::West => egui::vec2(-1.0, 0.0),
        Face::East => egui::vec2(1.0, 0.0),
        Face::South => egui::vec2(0.0, 1.0),
    }
}

const FACE_YELLOW: egui::Color32 = egui::Color32::from_rgb(0xf2, 0xc8, 0x3c);
const FACE_BLUE: egui::Color32 = egui::Color32::from_rgb(0x4f, 0x9d, 0xff);
const FACE_RED: egui::Color32 = egui::Color32::from_rgb(0xf0, 0x5c, 0x5c);
const FACE_GREEN: egui::Color32 = egui::Color32::from_rgb(0x5c, 0xd0, 0x6a);
const FACE_PINK: egui::Color32 = egui::Color32::from_rgb(0xe8, 0x7a, 0xc8);

/// The colour `layout`'s pads print on `face`'s button, so a character's colour
/// names the button that types it.
fn face_color(layout: PadLayout, face: Face) -> egui::Color32 {
    match (layout, face) {
        (PadLayout::Xbox, Face::North) | (PadLayout::Nintendo, Face::South) => FACE_YELLOW,
        (PadLayout::Xbox, Face::West)
        | (PadLayout::Nintendo, Face::North)
        | (PadLayout::PlayStation, Face::South) => FACE_BLUE,
        (_, Face::East) => FACE_RED,
        (PadLayout::Xbox, Face::South)
        | (PadLayout::Nintendo, Face::West)
        | (PadLayout::PlayStation, Face::North) => FACE_GREEN,
        (PadLayout::PlayStation, Face::West) => FACE_PINK,
    }
}

/// The character's colour on a `face_color` fill; yellow needs dark ink.
fn face_ink(color: egui::Color32) -> egui::Color32 {
    match color == FACE_YELLOW {
        true => egui::Color32::from_gray(0x20),
        false => egui::Color32::WHITE,
    }
}

/// Draw the space mark centred on `at`; Phosphor has no spacebar glyph.
fn paint_space_mark(painter: &egui::Painter, at: egui::Pos2, color: egui::Color32) {
    let half = egui::vec2(SPACE_MARK_W, SPACE_MARK_H) / 2.0;
    let points = vec![
        at + egui::vec2(-half.x, -half.y),
        at + egui::vec2(-half.x, half.y),
        at + egui::vec2(half.x, half.y),
        at + egui::vec2(half.x, -half.y),
    ];
    let stroke = egui::Stroke::new(SPACE_MARK_STROKE, color);
    painter.add(egui::Shape::line(points, stroke));
}

/// Draw the daisywheel: the groups on a ring, the aimed one lit, and the
/// centred face buttons' meanings on the hub. Returns the drawn height.
fn add_wheel(ctx: &egui::Context, osk: &Osk, layout: PadLayout, bottom_inset: f32) -> f32 {
    let side = 2.0 * (WHEEL_RADIUS + GROUP_RADIUS);
    let area = osk_area(bottom_inset).show(ctx, |ui| {
        panel().show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            let painter = ui.painter();
            let centre = rect.center();
            let text = |at, label: &str, size, color| {
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(size),
                    color,
                );
            };

            for sector in 0..WHEEL_SECTORS {
                let angle = sector as f32 / WHEEL_SECTORS as f32 * TAU;
                let at = centre + egui::vec2(angle.sin(), -angle.cos()) * WHEEL_RADIUS;
                let aimed = osk.sector() == Some(sector);
                // A ring rather than an accent fill, which the face colours
                // would not read on.
                let (fill, size) = match aimed {
                    true => (AIMED_FILL, AIMED_FONT),
                    false => (KEY_FILL, GROUP_FONT),
                };
                painter.circle_filled(at, GROUP_RADIUS, fill);
                if aimed {
                    painter.circle_stroke(at, GROUP_RADIUS, egui::Stroke::new(AIMED_RING, ACCENT));
                }
                for face in Face::ALL {
                    let button = at + face_dir(face) * FACE_OFFSET;
                    let (fill, ink) = match aimed {
                        true => {
                            let color = face_color(layout, face);
                            (color, face_ink(color))
                        }
                        false => (FACE_IDLE, egui::Color32::WHITE),
                    };
                    painter.circle_filled(button, FACE_RADIUS, fill);
                    let c = osk.wheel_char(sector, face).to_string();
                    text(button, &c, size, ink);
                }
            }

            painter.circle_stroke(centre, HUB_RADIUS, egui::Stroke::new(1.0, HINT));
            if osk.sector().is_none() {
                let places = osk.places();
                let at = |face| centre + face_dir(face) * FACE_OFFSET;
                let color = |face| face_color(layout, face);
                paint_space_mark(painter, at(places.y), color(places.y));
                text(at(places.x), bold::BACKSPACE, HUB_ICON, color(places.x));
                text(at(places.b), bold::X, HUB_ICON, color(places.b));
                let layer = osk.next_layer_label();
                text(at(places.a), layer, HUB_FONT, color(places.a));
            }

            let legend = |at, align, label: &str, color| {
                painter.text(
                    at,
                    align,
                    label,
                    egui::FontId::proportional(LEGEND_FONT),
                    color,
                );
            };
            let lang = osk.layout().name.to_uppercase();
            legend(rect.left_top(), egui::Align2::LEFT_TOP, &lang, HINT);
            legend(
                rect.right_top(),
                egui::Align2::RIGHT_TOP,
                "L1/R1 lang",
                HINT,
            );
            let shift = match osk.shift() || osk.caps {
                true => ACCENT,
                false => HINT,
            };
            legend(
                rect.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                "L2 shift",
                shift,
            );
            legend(
                rect.right_bottom(),
                egui::Align2::RIGHT_BOTTOM,
                "R2 enter",
                HINT,
            );
        });
    });
    area.response.rect.height()
}
