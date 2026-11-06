//! The gamepad cursor: [`AppUi`] owns its position and linger timer; the router
//! moves it, the render pass paints it via [`paint_cursor`].

use super::theme::palette;
use super::AppUi;
use crate::platform::window::AppWindow;
use egui_sdl2::egui;
use std::time::{Duration, Instant};

/// Gamepad cursor (logical px): an accent disc whose deep-teal rim reads on
/// light pages and whose white dot reads on accent-colored ones. The scroll
/// indicators share the fill and rim.
const CURSOR_RADIUS: f32 = 6.5;
const CURSOR_RIM: f32 = 1.0;
const CURSOR_DOT: f32 = 1.75;
const CURSOR_DOT_FILL: egui::Color32 = egui::Color32::WHITE;
/// The cursor's full painted half-extent — how far it reaches from its center,
/// used to keep the whole glyph (not just the center) inside the web view.
const CURSOR_EXTENT: f32 = CURSOR_RADIUS;

impl AppUi {
    /// Move the gamepad cursor by a logical-px delta and mark it visible, returning
    /// the part of the delta the window edge clipped. Clamped inset by the painted
    /// extent; it may roam over the toolbar so its buttons are clickable.
    #[inline]
    pub fn move_cursor(&mut self, dx: f32, dy: f32, window: &AppWindow) -> (f32, f32) {
        let (w, h) = window.size();
        // The window in points: the cursor is drawn in them, the window is pixels.
        let (w, h) = self.to_points(w as f32, h as f32);
        let want = (self.cursor.0 + dx, self.cursor.1 + dy);
        self.cursor.0 = want.0.clamp(CURSOR_EXTENT, w - CURSOR_EXTENT);
        self.cursor.1 = want.1.clamp(CURSOR_EXTENT, h - CURSOR_EXTENT);
        self.cursor_last_move = Some(Instant::now());
        (want.0 - self.cursor.0, want.1 - self.cursor.1)
    }

    /// Set the direction the cursor is edge-scrolling the page, per axis in
    /// -1..=1, or `(0, 0)` when it is not; drawn as arrows in place of the cursor.
    #[inline]
    pub fn set_edge_scroll(&mut self, dir: (i8, i8)) {
        self.edge_scroll = dir;
    }

    /// Whether the cursor is over the web view (below the toolbar). Clicks there
    /// go to the page; clicks above go to the egui toolbar via [`AppUi::click_ui`].
    #[inline]
    pub fn cursor_over_browser(&self) -> bool {
        // Anywhere off the toolbar chrome is the page; a hidden toolbar's rect
        // is off-screen, so the whole window is the page.
        !self
            .toolbar_rect
            .contains(egui::pos2(self.cursor.0, self.cursor.1))
    }

    /// Click the egui UI element under the cursor by feeding the backend a
    /// synthetic mouse button event (egui never sees the gamepad otherwise).
    /// `pressed` mirrors the A button's press/release so egui registers a click.
    pub fn click_ui(&mut self, pressed: bool, window: &mut AppWindow) {
        let ppp = self.egui_ctx.pixels_per_point();
        let (x, y) = ((self.cursor.0 * ppp) as i32, (self.cursor.1 * ppp) as i32);
        let window_id = window.sdl2_window().id();
        let event = if pressed {
            sdl2::event::Event::MouseButtonDown {
                timestamp: 0,
                window_id,
                which: 0,
                mouse_btn: sdl2::mouse::MouseButton::Left,
                clicks: 1,
                x,
                y,
            }
        } else {
            sdl2::event::Event::MouseButtonUp {
                timestamp: 0,
                window_id,
                which: 0,
                mouse_btn: sdl2::mouse::MouseButton::Left,
                clicks: 1,
                x,
                y,
            }
        };
        let _ = window.on_event(&event);
        self.repaint_pending = true;
    }

    /// Time left before the cursor auto-hides, or `None` if it's already hidden
    /// (never moved or idle past `cursor_linger`).
    #[inline]
    pub(super) fn cursor_visible_for(&self) -> Option<Duration> {
        self.cursor_last_move
            .and_then(|t| self.cursor_linger.checked_sub(t.elapsed()))
    }

    /// Set how long the gamepad cursor lingers after a move (the app calls this
    /// when the interface config changes live via the settings overlay).
    #[inline]
    pub fn set_cursor_linger(&mut self, ms: u64) {
        self.cursor_linger = Duration::from_millis(ms);
    }
}

/// Gamepad cursor overlay, always on top: the disc, or edge-scroll arrows
/// toward `edge_scroll`.
/// `pos` is in logical px, which equals egui points at the handheld's 1.0 scale.
pub(super) fn paint_cursor(ctx: &egui::Context, pos: egui::Pos2, edge_scroll: (i8, i8)) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("gamepad_cursor"),
    ));
    if edge_scroll != (0, 0) {
        add_edge_arrows(&painter, pos, edge_scroll);
    } else {
        painter.circle_filled(pos, CURSOR_RADIUS, palette().deep_teal);
        painter.circle_filled(pos, CURSOR_RADIUS - CURSOR_RIM, palette().accent);
        painter.circle_filled(pos, CURSOR_DOT, CURSOR_DOT_FILL);
    }
}

/// Edge-scroll arrows: one arrowhead per scrolling axis, pointing the way the
/// page moves. Sized to the circle's extent, which the edge clamp keeps on screen.
fn add_edge_arrows(painter: &egui::Painter, pos: egui::Pos2, (dx, dy): (i8, i8)) {
    let fill = palette().accent;
    let stroke = egui::Stroke::new(CURSOR_RIM, palette().deep_teal);
    let reach = CURSOR_EXTENT - CURSOR_RIM;
    for dir in [egui::vec2(dx as f32, 0.0), egui::vec2(0.0, dy as f32)] {
        if dir == egui::Vec2::ZERO {
            continue;
        }
        let side = dir.rot90() * reach;
        let base = pos - dir * reach;
        let points = vec![pos + dir * reach, base + side, base - side];
        painter.add(egui::Shape::convex_polygon(points, fill, stroke));
    }
}
