use super::sdl2_servo::{into_mouse_button_event, into_mouse_move_event};
use crate::app::AppCommand;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::ui::AppUi;
use crate::window::AppWindow;
use sdl2::controller::{Axis, Button};
use std::time::{Duration, Instant};

const AXIS_MAX: f32 = 32767.0;
/// Stick deflection below this (normalized) is treated as centered.
const DEADZONE: f32 = 0.25;
/// Trigger pull (normalized) above which L2/R2 count as pressed.
const TRIGGER_THRESHOLD: f32 = 0.5;
/// Stick deflection above which it counts as a directional press for OSK nav.
const OSK_NAV_THRESHOLD: f32 = 0.5;
/// Auto-repeat for stick-driven OSK navigation: delay before the first repeat,
/// then the interval between repeats while the stick is held.
const OSK_NAV_INITIAL_DELAY: Duration = Duration::from_millis(350);
const OSK_NAV_REPEAT: Duration = Duration::from_millis(140);
/// Cursor speed at full stick deflection, logical px per second.
const CURSOR_SPEED: f32 = 750.0;
/// Scroll speed at full stick deflection, device px per second.
const SCROLL_SPEED: f32 = 1600.0;

/// In-app gamepad handling: the left stick / D-pad drive a virtual cursor, the
/// right stick scrolls, and face/shoulder buttons map to clicks and navigation.
/// **X** opens the on-screen keyboard, after which the buttons drive it instead
/// (see [`Gamepad::on_button`]). On a handheld the pad is the only input device,
/// so this is the primary UI.
pub struct Gamepad {
    /// Left stick / D-pad vector, normalized and dead-zoned (-1..=1).
    left: (f32, f32),
    /// Right stick vector, normalized and dead-zoned (-1..=1).
    right: (f32, f32),
    /// Latched L2/R2 trigger states, for press-edge detection.
    l2_down: bool,
    r2_down: bool,
    /// Stick-driven OSK navigation: latched direction and next auto-repeat time.
    osk_nav_dir: (i32, i32),
    osk_nav_next: Instant,
    last_tick: Instant,
}

impl Gamepad {
    pub fn new() -> Self {
        Self {
            left: (0.0, 0.0),
            right: (0.0, 0.0),
            l2_down: false,
            r2_down: false,
            osk_nav_dir: (0, 0),
            osk_nav_next: Instant::now(),
            last_tick: Instant::now(),
        }
    }

    /// Whether the loop should keep ticking at ~60fps to animate the cursor/scroll.
    pub fn is_active(&self) -> bool {
        self.left != (0.0, 0.0) || self.right.1 != 0.0
    }

    pub fn on_axis(
        &mut self,
        axis: Axis,
        value: i16,
        ui: &mut AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // L2/R2 are throttle-style axes: drive the open keyboard's Shift/Enter on
        // the press edge so a single pull fires once.
        if matches!(axis, Axis::TriggerLeft | Axis::TriggerRight) {
            let pressed = value as f32 / AXIS_MAX > TRIGGER_THRESHOLD;
            let was = match axis {
                Axis::TriggerLeft => &mut self.l2_down,
                _ => &mut self.r2_down,
            };
            let rising = pressed && !*was;
            *was = pressed;
            if rising && ui.osk_visible() {
                match axis {
                    Axis::TriggerLeft => ui.osk_shift(),
                    _ => ui.osk_enter(browser, commands),
                }
            }
            return;
        }

        let v = value as f32 / AXIS_MAX;
        let v = if v.abs() < DEADZONE { 0.0 } else { v };
        match axis {
            Axis::LeftX => self.left.0 = v,
            Axis::LeftY => self.left.1 = v,
            Axis::RightX => self.right.0 = v,
            Axis::RightY => self.right.1 = v,
            _ => {}
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
        // While the keyboard is open, the buttons drive it, not the cursor:
        // D-pad moves the selection, A types it, X deletes, Y spaces, B closes
        // (L2/R2 shift/enter are handled in `on_axis`).
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
                Button::X => ui.osk_backspace(browser),
                Button::Y => ui.osk_space(browser),
                _ => {}
            }
            return;
        }

        // X opens the keyboard when it's closed.
        if button == Button::X && pressed {
            ui.osk_show();
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

        // While the keyboard is open, the left stick navigates its grid (with
        // key-style auto-repeat) instead of moving the cursor; scroll is frozen.
        if ui.osk_visible() {
            let dir = osk_nav_dir(self.left);
            if dir != self.osk_nav_dir {
                self.osk_nav_dir = dir;
                if dir != (0, 0) {
                    ui.osk_move(dir.0, dir.1);
                    self.osk_nav_next = now + OSK_NAV_INITIAL_DELAY;
                }
            } else if dir != (0, 0) && now >= self.osk_nav_next {
                ui.osk_move(dir.0, dir.1);
                self.osk_nav_next = now + OSK_NAV_REPEAT;
            }
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

/// Reduce a stick vector to a single discrete grid step along its dominant axis,
/// or `(0, 0)` when the stick is within the navigation dead zone.
fn osk_nav_dir(v: (f32, f32)) -> (i32, i32) {
    if v.0.abs().max(v.1.abs()) < OSK_NAV_THRESHOLD {
        (0, 0)
    } else if v.0.abs() >= v.1.abs() {
        (v.0.signum() as i32, 0)
    } else {
        (0, v.1.signum() as i32)
    }
}
