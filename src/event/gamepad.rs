use super::sdl2_servo::{into_mouse_button_event, into_mouse_move_event};
use crate::app::AppCommand;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::ui::AppUi;
use crate::window::AppWindow;
use sdl2::controller::{Axis, Button};
use std::time::Instant;

const AXIS_MAX: f32 = 32767.0;
/// Stick deflection below this (normalized) is treated as centered.
const DEADZONE: f32 = 0.25;
/// Cursor speed at full stick deflection, logical px per second.
const CURSOR_SPEED: f32 = 750.0;
/// Scroll speed at full stick deflection, device px per second.
const SCROLL_SPEED: f32 = 1600.0;

/// In-app gamepad handling: the left stick / D-pad drive a virtual cursor, the
/// right stick scrolls, and face/shoulder buttons map to clicks and navigation.
/// On a handheld the pad is the only input device, so this is the primary UI.
pub struct Gamepad {
    /// Left stick / D-pad vector, normalized and dead-zoned (-1..=1).
    left: (f32, f32),
    /// Right stick vector, normalized and dead-zoned (-1..=1).
    right: (f32, f32),
    last_tick: Instant,
}

impl Gamepad {
    pub fn new() -> Self {
        Self {
            left: (0.0, 0.0),
            right: (0.0, 0.0),
            last_tick: Instant::now(),
        }
    }

    /// Whether the loop should keep ticking at ~60fps to animate the cursor/scroll.
    pub fn is_active(&self) -> bool {
        self.left != (0.0, 0.0) || self.right.1 != 0.0
    }

    pub fn on_axis(&mut self, axis: Axis, value: i16) {
        let v = value as f32 / AXIS_MAX;
        let v = if v.abs() < DEADZONE { 0.0 } else { v };
        match axis {
            Axis::LeftX => self.left.0 = v,
            Axis::LeftY => self.left.1 = v,
            Axis::RightX => self.right.0 = v,
            Axis::RightY => self.right.1 = v,
            _ => {} // triggers unused for now
        }
    }

    /// Handle a controller button. Clicks/scrolls go straight to the page; nav
    /// actions are queued as [`AppCommand`]s.
    pub fn on_button(
        &mut self,
        button: Button,
        pressed: bool,
        ui: &mut AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // X toggles the on-screen keyboard regardless of mode.
        if button == Button::X && pressed {
            ui.toggle_osk();
            return;
        }

        // While the keyboard is open, the D-pad/A/B drive it, not the cursor.
        if ui.osk_visible() {
            if !pressed {
                return;
            }
            match button {
                Button::DPadLeft => ui.osk_move(-1, 0),
                Button::DPadRight => ui.osk_move(1, 0),
                Button::DPadUp => ui.osk_move(0, -1),
                Button::DPadDown => ui.osk_move(0, 1),
                Button::A => ui.osk_activate(browser, commands),
                Button::B => ui.osk_hide(),
                _ => {}
            }
            return;
        }

        match button {
            // A = left click at the cursor. Send a move first so Servo hit-tests
            // the right spot, then the button press/release.
            Button::A => {
                let (x, y) = ui.cursor_browser_rel();
                browser.handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
                let event = into_mouse_button_event(sdl2::mouse::MouseButton::Left, x, y, pressed);
                browser.handle_input(servo::InputEvent::MouseButton(event));
            }
            // D-pad mirrors the left stick so it also drives the cursor.
            Button::DPadLeft => self.left.0 = if pressed { -1.0 } else { 0.0 },
            Button::DPadRight => self.left.0 = if pressed { 1.0 } else { 0.0 },
            Button::DPadUp => self.left.1 = if pressed { -1.0 } else { 0.0 },
            Button::DPadDown => self.left.1 = if pressed { 1.0 } else { 0.0 },
            _ if !pressed => {} // remaining actions fire on press only
            Button::B | Button::LeftShoulder => {
                commands.push(AppCommand::Browser(BrowserCommand::Back))
            }
            Button::RightShoulder => commands.push(AppCommand::Browser(BrowserCommand::Foward)),
            Button::Start => commands.push(AppCommand::Browser(BrowserCommand::Reload)),
            _ => {}
        }
    }

    /// Advance the cursor and scroll by elapsed time, dispatching input to the page.
    pub fn tick(&mut self, window: &AppWindow, ui: &mut AppUi, browser: &AppBrowser) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        // While the keyboard is open, freeze cursor/scroll motion.
        if ui.osk_visible() {
            return;
        }

        if self.left != (0.0, 0.0) {
            ui.move_cursor(
                self.left.0 * CURSOR_SPEED * dt,
                self.left.1 * CURSOR_SPEED * dt,
                window,
            );
            let (x, y) = ui.cursor_browser_rel();
            browser.handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
        }

        if self.right.1 != 0.0 {
            // Stick down (+1) reveals lower content (positive Servo dy).
            let dy = self.right.1 * SCROLL_SPEED * dt;
            let (x, y) = ui.cursor_browser_rel();
            browser.scroll(0.0, dy, x, y);
        }
    }
}
