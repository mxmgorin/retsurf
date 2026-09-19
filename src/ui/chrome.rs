//! The toolbar-and-viewport chrome: position and auto-hide, the scroll that
//! drives the hide, the layout decided before the egui closure, and the SDL
//! resize fast path that must agree with it on the reserved strip.

use super::{AppUi, ChromeHidden, ToolbarLayout};
use crate::browser::AppBrowser;
use crate::config::ToolbarPosition;
use crate::platform::window::AppWindow;
use crate::ui::Focus;

impl AppUi {
    /// Move the toolbar to a window edge (live config change).
    #[inline]
    pub fn set_toolbar_position(&mut self, pos: ToolbarPosition) {
        self.toolbar_position = pos;
    }

    /// Enable/disable scroll-driven auto-hide (live config change). Re-showing
    /// the toolbar avoids leaving it stuck hidden from a prior scroll.
    #[inline]
    pub fn set_toolbar_autohide(&mut self, on: bool) {
        self.toolbar_autohide = on;
        if !on {
            self.toolbar_shown = true;
        }
    }

    /// Scroll the page and feed the toolbar auto-hide in one call — the only
    /// spelling of the pair, so a scroll cannot silently stop the auto-hide.
    pub fn scroll_page(&mut self, browser: &AppBrowser, dx: f32, dy: f32, x: f32, y: f32) {
        browser.scroll(dx, dy, x, y);
        self.notify_page_scroll(dy);
    }

    /// Feed a page-scroll delta (the same `dy` handed to [`AppBrowser::scroll`]:
    /// positive reveals lower content) so the toolbar can hide on scroll-down and
    /// reveal on scroll-up. Accumulates to a threshold; a no-op without auto-hide.
    fn notify_page_scroll(&mut self, dy: f32) {
        if !self.toolbar_autohide || dy == 0.0 {
            return;
        }
        // Reset the accumulator on a direction change so a flick the other way
        // responds immediately instead of cancelling out a long prior drag.
        if (dy > 0.0) != (self.scroll_accum > 0.0) {
            self.scroll_accum = 0.0;
        }
        self.scroll_accum += dy;
        const THRESHOLD: f32 = 48.0;
        if self.scroll_accum > THRESHOLD {
            self.toolbar_shown = false;
            self.scroll_accum = 0.0;
        } else if self.scroll_accum < -THRESHOLD {
            self.toolbar_shown = true;
            self.scroll_accum = 0.0;
        }
    }

    /// Resize the browser to the web-view area on SDL window-resize events:
    /// egui's reactive sizing reads the central rect a frame later. Shares
    /// `browser_viewport` with [`AppUi::update`], so the two never double-resize.
    pub fn resize_browser(&mut self, window: &AppWindow, browser: &AppBrowser) {
        let (dw, dh) = window.drawable_size();
        if dw == 0 || dh == 0 {
            return;
        }
        // An auto-hide bar floats as an overlay on either edge, so the web view
        // stays full-height; without auto-hide the bar reserves a strip.
        let overlay = self.toolbar_autohide;
        let toolbar_px = if overlay {
            0
        } else {
            (self.toolbar_height * self.egui_ctx.pixels_per_point()).round() as u32
        };
        let size = (dw, dh.saturating_sub(toolbar_px).max(1));
        if size != self.browser_viewport {
            self.browser_viewport = size;
            self.forced_passes = self.forced_passes.max(1);
            browser.resize(size.0, size.1);
        }
    }

    /// Decide the toolbar layout before the egui closure (these reads borrow all
    /// of `self`, which can't overlap `egui.run`). Auto-hide floats the bar: a
    /// strip that came and went would resize the web view, a full Servo reflow.
    pub(super) fn toolbar_layout(&self, chrome_hidden: ChromeHidden) -> ToolbarLayout {
        // Game Mode is the exception: nothing it opens types into the chrome,
        // so an overlay of its own must not bring the bar back over the game.
        let typing = self.focus() != Focus::Page && !chrome_hidden.game_mode;
        ToolbarLayout {
            position: self.toolbar_position,
            // Typing still wins: auto-hide forces the bar up for a focused field,
            // and a hidden chrome must not leave an invisible address bar to type into.
            shown: typing
                || (!chrome_hidden.any() && (!self.toolbar_autohide || self.toolbar_shown)),
            overlay: self.toolbar_autohide,
        }
    }

    #[inline]
    pub(super) fn is_pointer_over_toolbar(&self, window: &AppWindow) -> bool {
        let Some(pos) = window.pointer_pos_in_points() else {
            return false;
        };
        self.toolbar_rect.contains(pos)
    }

    /// Whether a *pixel*-space y (raw SDL finger events) lands in the web view:
    /// touches over the toolbar are egui's, and start no page gesture.
    #[inline]
    pub fn point_over_webview(&self, y_px: f32) -> bool {
        let y = y_px / self.egui_ctx.pixels_per_point();
        !self.toolbar_rect.y_range().contains(y)
    }
}
