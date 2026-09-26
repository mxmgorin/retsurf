//! Rendering of the wheel: a translucent disc with the character groups on
//! a ring, the hub's centred-stick actions, and the other buttons' tabs on the
//! rim. Colours and sides follow the pad layout.

use super::osk_area;
use crate::config::{Face, PadLayout};
use crate::overlay::osk::wheel::{FACES, WHEEL_SECTORS};
use crate::overlay::osk::Osk;
use crate::ui::theme::ACCENT;
use egui_phosphor::bold;
use egui_sdl2::egui;
use std::f32::consts::TAU;

/// The wheel's geometry: group centres sit on `WHEEL_RADIUS`, each group's
/// characters on a `FACE_RADIUS` button `FACE_OFFSET` out toward their face.
const WHEEL_RADIUS: f32 = 92.0;
const GROUP_RADIUS: f32 = 32.0;
const FACE_OFFSET: f32 = 16.0;
const FACE_RADIUS: f32 = 9.5;
/// The band past the groups that carries the shoulder-button tabs.
const RIM_BAND: f32 = 18.0;
const RIM_RADIUS: f32 = WHEEL_RADIUS + GROUP_RADIUS + RIM_BAND;
/// Air between the disc and the bottom edge.
const WHEEL_MARGIN: f32 = 8.0;
const TAB_RADIUS: f32 = RIM_RADIUS - RIM_BAND / 2.0;
const TAB_WIDTH: f32 = RIM_BAND - 4.0;
/// A tab's arc either side of its label, in radians.
const TAB_HALF_SPAN: f32 = 0.27;
const TAB_SEGMENTS: usize = 12;
/// Where the shoulder and trigger tabs sit, clockwise from up (mirrored left).
const SHOULDER_TAB: f32 = TAU * 0.1;
const TRIGGER_TAB: f32 = TAU * 0.2;
/// Select's and Start's tabs, either side of the bottom.
const BOTTOM_TAB: f32 = TAU * 0.05;
/// The hub disk, clear of the groups, and its marks' distance from the centre.
const HUB_GAP: f32 = 6.0;
const HUB_RADIUS: f32 = WHEEL_RADIUS - GROUP_RADIUS - HUB_GAP;
const HUB_MARK_OFFSET: f32 = 26.0;
/// The aimed-group pointer: a triangle just inside the hub's edge.
const POINTER_INSET: f32 = 8.0;
const POINTER_LENGTH: f32 = 7.0;
const POINTER_HALF_WIDTH: f32 = 4.5;
const GROUP_FONT: f32 = 13.0;
const AIMED_FONT: f32 = 15.0;
const TAB_FONT: f32 = 10.0;
const HUB_ICON: f32 = 16.0;
/// The drawn space mark, a bracket open upward.
const SPACE_MARK_W: f32 = 12.0;
const SPACE_MARK_H: f32 = 4.0;
const SPACE_MARK_STROKE: f32 = 1.5;
const RIM_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(0x1c, 0x1c, 0x1e, 0xe0);
const HUB_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(0x34, 0x34, 0x36, 0xa0);
const GROUP_FILL: egui::Color32 = egui::Color32::from_gray(0x48);
const AIMED_FILL: egui::Color32 = egui::Color32::from_gray(0x62);
/// A face button outside the aimed group: dark, so only the aimed one reads.
const FACE_IDLE: egui::Color32 = egui::Color32::from_gray(0x2c);
const TAB_FILL: egui::Color32 = egui::Color32::from_gray(0x36);
/// A tab's action, dimmer than its button so the two read as a pair.
const TAB_ACTION: egui::Color32 = egui::Color32::from_gray(0xa8);
const TAB_LABEL_GAP: f32 = 5.0;
/// Hub marks while a group is aimed.
const HUB_DIM: egui::Color32 = egui::Color32::from_gray(0x80);

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

/// The colour a `layout` pad prints on `face`'s button.
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

/// Unit vector toward `angle` radians clockwise from up, in screen space.
fn clock(angle: f32) -> egui::Vec2 {
    egui::vec2(angle.sin(), -angle.cos())
}

/// A tab on the rim at `angle`: an arc with the button and its action along it.
fn paint_tab(
    painter: &egui::Painter,
    centre: egui::Pos2,
    angle: f32,
    (button, action): (&str, &str),
    fill: egui::Color32,
) {
    let arc: Vec<egui::Pos2> = (0..=TAB_SEGMENTS)
        .map(|i| {
            let t = i as f32 / TAB_SEGMENTS as f32;
            let a = angle - TAB_HALF_SPAN + 2.0 * TAB_HALF_SPAN * t;
            centre + clock(a) * TAB_RADIUS
        })
        .collect();
    painter.add(egui::Shape::line(arc, egui::Stroke::new(TAB_WIDTH, fill)));

    // Tangent to the rim, and turned over on the lower half so it reads upright.
    let upright = match angle.cos() < 0.0 {
        true => angle + TAU / 2.0,
        false => angle,
    };
    let format = |color| egui::TextFormat {
        font_id: egui::FontId::proportional(TAB_FONT),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(button, 0.0, format(egui::Color32::WHITE));
    job.append(action, TAB_LABEL_GAP, format(TAB_ACTION));
    let galley = painter.layout_job(job);
    let at = centre + clock(angle) * TAB_RADIUS - galley.size() / 2.0;
    let shape = egui::epaint::TextShape::new(at, galley, egui::Color32::WHITE)
        .with_angle_and_anchor(upright, egui::Align2::CENTER_CENTER);
    painter.add(shape);
}

/// Draw the wheel; returns the drawn rect.
pub(super) fn add_wheel(
    ctx: &egui::Context,
    osk: &Osk,
    layout: PadLayout,
    bottom_inset: f32,
) -> egui::Rect {
    let side = 2.0 * RIM_RADIUS;
    let area = osk_area(osk, bottom_inset + WHEEL_MARGIN).show(ctx, |ui| {
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

        painter.circle_filled(centre, RIM_RADIUS, RIM_FILL);
        painter.circle_filled(centre, HUB_RADIUS, HUB_FILL);

        let sector_angle = |sector: usize| sector as f32 / WHEEL_SECTORS as f32 * TAU;
        for sector in 0..WHEEL_SECTORS {
            let at = centre + clock(sector_angle(sector)) * WHEEL_RADIUS;
            let aimed = osk.sector() == Some(sector);
            let (fill, size) = match aimed {
                true => (AIMED_FILL, AIMED_FONT),
                false => (GROUP_FILL, GROUP_FONT),
            };
            painter.circle_filled(at, GROUP_RADIUS, fill);
            for face in FACES {
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

        if let Some(sector) = osk.sector() {
            let dir = clock(sector_angle(sector));
            let tip = centre + dir * (HUB_RADIUS - POINTER_INSET);
            let side = egui::vec2(-dir.y, dir.x) * POINTER_HALF_WIDTH;
            let base = tip - dir * POINTER_LENGTH;
            painter.add(egui::Shape::convex_polygon(
                vec![tip, base + side, base - side],
                egui::Color32::WHITE,
                egui::Stroke::NONE,
            ));
        }

        // The centred stick's actions, coloured while they are what a press does.
        let places = layout.places();
        let at = |face| centre + face_dir(face) * HUB_MARK_OFFSET;
        let color = |face| match osk.sector() {
            None => face_color(layout, face),
            Some(_) => HUB_DIM,
        };
        paint_space_mark(painter, at(places.y), color(places.y));
        text(at(places.x), bold::BACKSPACE, HUB_ICON, color(places.x));
        text(at(places.b), bold::X, HUB_ICON, color(places.b));
        text(at(places.a), bold::KEY_RETURN, HUB_ICON, color(places.a));

        // Triggers outside, shoulders inside, each on its own side of the pad.
        let shift = match osk.shift() || osk.caps {
            true => ACCENT,
            false => TAB_FILL,
        };
        paint_tab(painter, centre, -TRIGGER_TAB, ("L2", "SHIFT"), shift);
        paint_tab(painter, centre, -SHOULDER_TAB, ("L1", "DEL"), TAB_FILL);
        paint_tab(painter, centre, SHOULDER_TAB, ("R1", "SPACE"), TAB_FILL);
        let digits = match osk.digits() {
            true => ACCENT,
            false => TAB_FILL,
        };
        paint_tab(painter, centre, TRIGGER_TAB, ("R2", "NUM"), digits);
        let lang = osk.layout().name.to_uppercase();
        let (select, start) = (TAU / 2.0 + BOTTOM_TAB, TAU / 2.0 - BOTTOM_TAB);
        paint_tab(painter, centre, select, ("SELECT", &lang), TAB_FILL);
        paint_tab(painter, centre, start, ("START", "TAB"), TAB_FILL);
    });
    area.response.rect
}
