//! Driving the page on [`AppBrowser`]: pointer/scroll input, and the evaluated
//! scripts that ask the page for what the embedder API cannot (hint rects,
//! clearing a field, scrolling the focused element clear of the OSK).

use super::AppBrowser;
use crate::event::user::UserEvent;
use crate::overlay::hints::Hint;

impl AppBrowser {
    /// A web-view point in the pixels the page is rendered and hit-tested in.
    #[inline]
    fn page_px(&self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.inner.hidpi.get();
        (x * scale, y * scale)
    }

    /// Point the page at `(x, y)` (web-view points), so `:hover` and JS follow.
    pub fn mouse_move(&self, x: f32, y: f32) {
        let (x, y) = self.page_px(x, y);
        self.handle_input(servo::InputEvent::MouseMove(
            crate::event::sdl2_servo::into_mouse_move_event(x, y),
        ));
    }

    /// Press or release `button` at `(x, y)` (web-view points).
    pub fn mouse_button(&self, button: sdl2::mouse::MouseButton, x: f32, y: f32, down: bool) {
        let (x, y) = self.page_px(x, y);
        self.handle_input(servo::InputEvent::MouseButton(
            crate::event::sdl2_servo::into_mouse_button_event(button, x, y, down),
        ));
    }

    /// The DOM `wheel` event at `(x, y)` (web-view points). Fires handlers only —
    /// [`Self::scroll`] is what moves the page.
    pub fn wheel(&self, dx: i32, dy: i32, x: f32, y: f32) {
        let (x, y) = self.page_px(x, y);
        self.handle_input(servo::InputEvent::Wheel(
            crate::event::sdl2_servo::into_wheel_event(dx, dy, x, y),
        ));
    }

    pub fn handle_input(&self, event: servo::InputEvent) {
        let Some(tab) = self.inner.active_webview() else {
            return;
        };

        tab.notify_input_event(event.clone());

        if let servo::InputEvent::MouseButton(be) = event {
            if be.action == servo::MouseButtonAction::Down {
                match be.button {
                    servo::MouseButton::Back => _ = tab.go_back(1),
                    servo::MouseButton::Forward => _ = tab.go_forward(1),
                    _ => {}
                }
            }
        }
    }

    /// Scroll the active page by a delta at `(x, y)`, both in web-view points.
    /// Positive `dy` reveals content lower on the page. This is the native
    /// compositor scroll (`InputEvent::Wheel` only fires the DOM event).
    pub fn scroll(&self, dx: f32, dy: f32, x: f32, y: f32) {
        let Some(tab) = self.inner.active_webview() else {
            return;
        };
        let (dx, dy) = self.page_px(dx, dy);
        let (x, y) = self.page_px(x, y);
        let delta = servo::Scroll::Delta(servo::DeviceVector2D::new(dx, dy).into());
        let point = servo::DevicePoint::new(x, y).into();
        tab.notify_scroll_event(delta, point);
    }

    /// Empty the page's focused field (the keyboard's Clr key), notifying the
    /// page as a real edit would.
    pub fn clear_focused_field(&self) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        webview.evaluate_javascript(CLEAR_FIELD_JS, |_| {});
    }

    /// Scroll the page so its focused element clears the bottom `covered`
    /// fraction of the viewport — the on-screen keyboard, which the page knows
    /// nothing about. In-page because only the document knows where its focused
    /// element is; Servo's IME rect is frame-relative CSS (its own FIXME).
    pub fn lift_focus_above(&self, covered: f32) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        let js = LIFT_FOCUS_JS.replace("COVERED", &format!("{covered:.4}"));
        webview.evaluate_javascript(js, |_| {});
    }

    /// Ask the active page for its visible clickable elements (hint mode). Runs
    /// asynchronously; the rects land in `hint_rects` (drained via
    /// [`Self::take_hint_rects`]) and a wake-up event is sent. An evaluation
    /// error yields an empty list, which exits hint mode.
    pub fn collect_hints(&self) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        let inner = self.inner.clone();
        webview.evaluate_javascript(COLLECT_HINTS_JS, move |result| {
            let mut hints = vec![];
            match result {
                // The script returns a flat array, five entries per element:
                // x, y, w, h (viewport-relative CSS px) then the link's URL.
                Ok(servo::JSValue::Array(values)) => {
                    let num = |v: &servo::JSValue| match v {
                        servo::JSValue::Number(n) => Some(*n as f32),
                        _ => None,
                    };
                    for c in values.as_chunks::<5>().0 {
                        let (Some(x), Some(y), Some(w), Some(h)) =
                            (num(&c[0]), num(&c[1]), num(&c[2]), num(&c[3]))
                        else {
                            continue;
                        };
                        let url = match &c[4] {
                            servo::JSValue::String(s) if !s.is_empty() => Some(s.clone()),
                            _ => None,
                        };
                        hints.push(Hint { x, y, w, h, url });
                    }
                }
                Ok(other) => log::warn!("hint collection returned unexpected value: {other:?}"),
                Err(e) => log::warn!("hint collection failed: {e:?}"),
            }
            *inner.hint_rects.borrow_mut() = Some(hints);
            inner.event_sender.send(UserEvent::HintsReady);
        });
    }

    /// Take the rects from the last hint collection, if it has finished since
    /// the previous call. Drained once per frame by the main loop.
    #[inline]
    pub fn take_hint_rects(&self) -> Option<Vec<Hint>> {
        self.inner.hint_rects.borrow_mut().take()
    }
}

/// Empty the focused field, firing the events a page listens for.
const CLEAR_FIELD_JS: &str = include_str!("assets/clear_field.js");

/// Scroll the focused element clear of the viewport's bottom `COVERED` fraction
/// (substituted by [`AppBrowser::lift_focus_above`]). A fraction, not a pixel
/// count, so it needs no CSS-px / logical-px conversion.
const LIFT_FOCUS_JS: &str = include_str!("assets/lift_focus.js");

/// Collect the visible clickable elements as a flat `[x, y, w, h, …]` array
/// (viewport-relative CSS px). Skips off-viewport, zero-size, hidden, and
/// click-through elements; capped so a link-farm page can't flood the IPC
/// channel. Cross-origin iframes are unreachable from the top document — their
/// content gets no hints (the virtual cursor remains the fallback).
const COLLECT_HINTS_JS: &str = include_str!("assets/collect_hints.js");
