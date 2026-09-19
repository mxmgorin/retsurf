//! The paint-and-viewport seam: spinning Servo's event loop, painting the
//! active webview into the rendering context, and keeping the viewport, DPI
//! and screen geometry in step with the window.

use super::AppBrowser;

impl AppBrowser {
    /// Spin the Servo event loop once, running delegate callbacks and updating paint output.
    #[inline]
    pub fn pump_event_loop(&self) {
        self.inner.servo.spin_event_loop();
    }

    /// Paint the contents of the active WebView into its RenderingContext. Returns true if a paint was performed.
    pub fn paint(&self) -> bool {
        if !self.inner.repaint_pending.get() {
            return false;
        }

        if let Some(tab) = self.inner.active_webview() {
            self.inner.repaint_pending.set(false);
            tab.paint();
            return true;
        }

        false
    }

    /// Tell the page which panel it is on and where the window sits on it. In
    /// device pixels; Servo divides by the webview's ratio for the CSS values.
    pub fn set_screen_geometry(&self, screen: (u32, u32), window: (i32, i32, u32, u32)) {
        let (width, height) = screen;
        let (x, y, window_width, window_height) = window;
        self.inner.screen.set(servo::ScreenGeometry {
            size: euclid::Size2D::new(width as i32, height as i32),
            // No docks or system bars on any target we ship to.
            available_size: euclid::Size2D::new(width as i32, height as i32),
            window_rect: euclid::Box2D::from_origin_and_size(
                euclid::Point2D::new(x, y),
                euclid::Size2D::new(window_width as i32, window_height as i32),
            ),
        });
    }

    /// Follow the chrome's zoom with the page's device pixel ratio. Every open
    /// tab, not just the active one: a hidden tab would lay out for the old scale.
    pub fn set_hidpi(&self, scale: f32) {
        if self.inner.hidpi.replace(scale) == scale {
            return;
        }
        for tab in self.inner.tabs.borrow().iter() {
            tab.webview
                .set_hidpi_scale_factor(euclid::Scale::new(scale));
        }
    }

    pub fn resize(&self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let size = dpi::PhysicalSize::new(w, h);
        // A full reflow each, so the count is the measurement when chrome
        // that comes and goes is suspected of resizing the page.
        log::debug!("viewport resize: {w}x{h}");
        // Servo's resize reflows *and* resizes the context, but early-returns on
        // a matching size, so resizing the context first skipped the reflow.
        match self.inner.active_webview() {
            Some(tab) => tab.resize(size),
            None => self.inner.rendering_ctx.resize(size),
        }
    }
}
