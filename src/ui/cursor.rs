//! The gamepad cursor: [`AppUi`] owns its position and linger timer; the router
//! moves it, the render pass paints it (or the scroll-mode indicator) via
//! [`paint_cursor`].

use super::AppUi;
use crate::platform::window::AppWindow;
use egui_sdl2::egui;
use std::time::{Duration, Instant};

/// Gamepad cursor overlay: circle radius and outline width (logical px).
const CURSOR_RADIUS: f32 = 5.0;
const CURSOR_STROKE: f32 = 1.5;
/// The cursor's full painted half-extent — how far it reaches from its center,
/// used to keep the whole glyph (not just the center) inside the web view.
const CURSOR_EXTENT: f32 = CURSOR_RADIUS + CURSOR_STROKE / 2.0;

impl AppUi {
    /// Move the gamepad cursor by a logical-px delta and mark it visible. Clamped
    /// to the window (inset by the cursor's painted extent so the whole circle
    /// stays on screen); it may roam over the toolbar so its buttons are clickable.
    #[inline]
    pub fn move_cursor(&mut self, dx: f32, dy: f32, window: &AppWindow) {
        let (w, h) = window.size();
        // The window in points: the cursor is drawn in them, the window is pixels.
        let (w, h) = self.to_points(w as f32, h as f32);
        self.cursor.0 = (self.cursor.0 + dx).clamp(CURSOR_EXTENT, w - CURSOR_EXTENT);
        self.cursor.1 = (self.cursor.1 + dy).clamp(CURSOR_EXTENT, h - CURSOR_EXTENT);
        self.cursor_last_move = Some(Instant::now());
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

    /// The gamepad cursor in browser-relative coordinates (below the toolbar),
    /// ready to feed to Servo as a mouse position.
    #[inline]
    pub fn cursor_browser_rel(&self) -> (f32, f32) {
        self.to_browser_rel_pos(self.cursor.0, self.cursor.1)
    }

    /// Set how long the gamepad cursor lingers after a move (the app calls this
    /// when the interface config changes live via the settings overlay).
    #[inline]
    pub fn set_cursor_linger(&mut self, ms: u64) {
        self.cursor_linger = Duration::from_millis(ms);
    }

    /// Mirror the gamepad's latched D-pad scroll mode (router, every analog
    /// frame). Entering pings the linger timer so the indicator shows like the
    /// cursor, then auto-hides unless [`Self::mark_cursor_active`] keeps it alive.
    #[inline]
    pub fn set_scroll_mode(&mut self, on: bool) {
        if on && !self.scroll_mode {
            self.cursor_last_move = Some(Instant::now());
        }
        self.scroll_mode = on;
    }

    /// Refresh the linger timer without moving the cursor: active page scroll
    /// keeps the scroll-mode indicator visible, then it hides like the cursor.
    #[inline]
    pub fn mark_cursor_active(&mut self) {
        self.cursor_last_move = Some(Instant::now());
    }
}

/// Gamepad cursor overlay, always on top: the circle, or the scroll-mode
/// indicator while D-pad scroll is latched. `pos` is in logical px, which equals
/// egui points at the handheld's 1.0 scale factor.
pub(super) fn paint_cursor(ctx: &egui::Context, pos: egui::Pos2, scroll_mode: bool) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("gamepad_cursor"),
    ));
    if scroll_mode {
        // Same linger/auto-hide as the cursor: shown while scrolling, then fades.
        add_scroll_indicator(&painter, pos);
    } else {
        painter.circle_filled(pos, CURSOR_RADIUS, egui::Color32::from_white_alpha(235));
        painter.circle_stroke(
            pos,
            CURSOR_RADIUS,
            egui::Stroke::new(CURSOR_STROKE, egui::Color32::BLACK),
        );
    }
}

/// The D-pad scroll-mode indicator at the parked cursor position: a center dot
/// with up/down arrowheads, like a browser's middle-click autoscroll marker.
fn add_scroll_indicator(painter: &egui::Painter, pos: egui::Pos2) {
    let fill = egui::Color32::from_white_alpha(235);
    let stroke = egui::Stroke::new(CURSOR_STROKE, egui::Color32::BLACK);
    painter.circle_filled(pos, 2.5, fill);
    painter.circle_stroke(pos, 2.5, stroke);
    for dir in [-1.0f32, 1.0] {
        let tip = egui::pos2(pos.x, pos.y + dir * 12.0);
        let base = pos.y + dir * 5.5;
        let points = vec![
            tip,
            egui::pos2(pos.x - 4.5, base),
            egui::pos2(pos.x + 4.5, base),
        ];
        painter.add(egui::Shape::convex_polygon(points, fill, stroke));
    }
}
