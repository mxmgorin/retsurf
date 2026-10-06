//! The frame shown while the engine starts: the app icon's sun setting on its
//! waves, the wordmark on the sea. One frame, so it paints straight into a root
//! `Ui` rather than an area, whose first show is an invisible sizing pass.

use super::brand::{self, Horizon};
use super::home;
use super::theme::BG;
use egui_sdl2::egui;

/// The sun's radius as a share of the screen's shorter side.
const SUN_SHARE: f32 = 0.11;
/// The horizon's height as a share of the screen, from the top.
const HORIZON_AT: f32 = 0.58;
/// The wordmark's type size, and its drop below the near wave, as shares of the
/// sun's radius.
const MARK_SIZE: f32 = 0.68;
const MARK_GAP: f32 = 0.3;

pub fn add_splash(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("splash"),
        egui::UiBuilder::new().max_rect(screen),
    );
    ui.painter().rect_filled(screen, 0.0, BG);
    let sun = Horizon {
        radius: screen.width().min(screen.height()) * SUN_SHARE,
        sink: 0.3,
        dim: 0.0,
    };
    let horizon = screen.top() + screen.height() * HORIZON_AT;
    brand::paint_horizon(
        ui.painter(),
        screen.x_range(),
        horizon,
        screen.bottom(),
        screen.center().x,
        &sun,
    );
    let size = sun.radius * MARK_SIZE;
    let mark = home::wordmark_text_size(&ui, size);
    let mark_top = horizon + sun.depth() + sun.radius * MARK_GAP;
    let left = screen.center().x - mark.x / 2.0;
    home::paint_wordmark_text(&ui, egui::pos2(left, mark_top), size);
}
