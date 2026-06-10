//! The central input router: decides what a contextual [`InputCommand`] does
//! given the current state. This is where the "keyboard open? menu open? cursor
//! over the page or toolbar?" branches live — the gamepad itself stays
//! state-agnostic and only emits intents.

use super::{App, AppCommand, InputCommand};
use crate::browser::BrowserCommand;
use crate::event::sdl2_servo::{into_mouse_button_event, into_mouse_move_event};
use crate::osk::OskCommand;
use std::time::{Duration, Instant};

impl App {
    /// Route one contextual input intent.
    pub(super) fn route_input(&mut self, command: &InputCommand, out: &mut Vec<AppCommand>) {
        match command {
            InputCommand::Primary(pressed) => {
                if self.ui.menu_visible() {
                    if *pressed {
                        self.menu_open_selected();
                    }
                } else if !self.ui.osk_visible() && self.ui.hints_visible() {
                    if *pressed {
                        self.activate_hint();
                    }
                } else {
                    self.primary_action(*pressed, out);
                }
            }
            InputCommand::Cancel => {
                if self.ui.menu_visible() {
                    self.ui.menu_close();
                } else if self.ui.osk_visible() {
                    self.ui.osk(OskCommand::Hide, &self.browser, out);
                } else if self.ui.hints_visible() {
                    self.ui.hints_hide();
                } else {
                    self.browser
                        .execute_command(&BrowserCommand::Back, &self.config.browser);
                }
            }
            InputCommand::ToggleOsk => {
                if self.ui.menu_visible() {
                    // X deletes the highlighted entry (closes a tab in the Tabs section).
                    self.delete_menu_selection();
                } else {
                    // The keyboard takes over the stick and A — leave hint mode.
                    self.ui.hints_hide();
                    let cmd = if self.ui.osk_visible() {
                        OskCommand::Backspace
                    } else {
                        OskCommand::Show
                    };
                    self.ui.osk(cmd, &self.browser, out);
                }
            }
            // L3: toggle link-hint navigation (collection is asynchronous — the
            // badges appear once the page reports its clickable elements). Inert
            // under the menu/keyboard overlays, which own the stick and A.
            InputCommand::Hints => {
                if self.ui.menu_visible() || self.ui.osk_visible() {
                } else if self.ui.hints_visible() {
                    self.ui.hints_hide();
                } else {
                    self.ui.hints_begin_collect();
                    self.browser.collect_hints();
                }
            }
            // Dedicated keyboard keys act only while the keyboard is open. The one
            // exception is Y (Space): outside the keyboard it reloads the page.
            InputCommand::Shoulder(delta) => {
                if self.ui.menu_visible() {
                    self.ui.menu_switch(*delta);
                } else {
                    let cmd = if *delta < 0 {
                        BrowserCommand::Back
                    } else {
                        BrowserCommand::Foward
                    };
                    self.browser.execute_command(&cmd, &self.config.browser);
                }
            }
            InputCommand::Trigger { right, pressed } => {
                if self.ui.osk_visible() {
                    // Keyboard: L2 is a held Shift, R2 is Enter on the press edge.
                    if *right {
                        if *pressed {
                            self.ui.osk(OskCommand::Enter, &self.browser, out);
                        }
                    } else {
                        self.ui.osk(OskCommand::Shift(*pressed), &self.browser, out);
                    }
                } else if *pressed {
                    // Quick tab switch: L2 previous, R2 next (wraps).
                    self.browser.cycle_tab(if *right { 1 } else { -1 });
                }
            }
            InputCommand::Osk(cmd) => {
                if self.ui.osk_visible() {
                    self.ui.osk(*cmd, &self.browser, out);
                } else if matches!(cmd, OskCommand::Space) {
                    self.browser
                        .execute_command(&BrowserCommand::Reload, &self.config.browser);
                }
            }
            InputCommand::Analog { aim, scroll } => self.route_analog(*aim, *scroll, out),
        }
    }

    /// Click the selected hint: a synthetic mouse move + press + release at its
    /// center (so JS click handlers fire like for a real click), then leave hint
    /// mode — the click usually navigates, invalidating the rects anyway.
    fn activate_hint(&mut self) {
        let Some((x, y)) = self.ui.hints_selected_center() else {
            self.ui.hints_hide();
            return;
        };
        self.ui.hints_hide();
        self.browser
            .handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
        for pressed in [true, false] {
            let event = into_mouse_button_event(sdl2::mouse::MouseButton::Left, x, y, pressed);
            self.browser
                .handle_input(servo::InputEvent::MouseButton(event));
        }
    }

    /// The A button: activate the selected keyboard key, click the page in Servo,
    /// or click the egui toolbar — whichever the cursor is currently over.
    fn primary_action(&mut self, pressed: bool, out: &mut Vec<AppCommand>) {
        if self.ui.osk_visible() {
            if pressed {
                self.ui.osk(OskCommand::Activate, &self.browser, out);
            }
        } else if self.ui.cursor_over_browser() {
            let (x, y) = self.ui.cursor_browser_rel();
            self.browser
                .handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
            let event = into_mouse_button_event(sdl2::mouse::MouseButton::Left, x, y, pressed);
            self.browser
                .handle_input(servo::InputEvent::MouseButton(event));
        } else {
            self.ui.click_ui(pressed, &self.window);
        }
    }

    /// Apply per-frame analog state: keyboard grid navigation (with auto-repeat)
    /// while the keyboard is open, otherwise cursor movement and page scroll.
    fn route_analog(&mut self, aim: (f32, f32), scroll: f32, out: &mut Vec<AppCommand>) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;
        // The loop blocks on input while idle, so the first frame after a press
        // sees the whole idle gap as `dt`. Integrating that teleports the cursor
        // (a D-pad tap jumps ~`cursor_speed * dt`), so treat any over-long frame
        // as a fresh start: no motion this frame, normal motion from the next.
        let dt = if dt > 0.1 { 0.0 } else { dt };
        // Scalar copies: the config holds non-Copy data (the bindings map), so
        // it can't be borrowed across the `&mut self` calls below.
        let cfg = &self.config.gamepad;
        let (cursor_speed, scroll_speed, nav_threshold) =
            (cfg.cursor_speed, cfg.scroll_speed, cfg.osk_nav_threshold);

        // The menu: left/right switches section, up/down moves the selection
        // (dominant axis only, so a diagonal nudge does just one thing).
        if self.ui.menu_visible() {
            let dir = osk_nav_dir(aim, nav_threshold);
            if self.nav_repeat(dir, now) {
                if dir.0 != 0 {
                    self.ui.menu_switch(dir.0);
                } else if dir.1 != 0 {
                    self.ui.menu_move(dir.1);
                }
            }
            return;
        }

        // The keyboard: the stick navigates the key grid.
        if self.ui.osk_visible() {
            let dir = osk_nav_dir(aim, nav_threshold);
            if self.nav_repeat(dir, now) {
                self.ui.osk(OskCommand::Move(dir.0, dir.1), &self.browser, out);
            }
            return;
        }

        // Hint mode: the stick hops between hints; the right stick still scrolls
        // (badges go stale as the page moves — schedule a re-collect).
        if self.ui.hints_visible() {
            let dir = osk_nav_dir(aim, nav_threshold);
            if self.nav_repeat(dir, now) && dir != (0, 0) {
                self.ui.hints_move(dir);
            }
            if scroll != 0.0 {
                let dy = scroll * scroll_speed * dt;
                let (x, y) = self
                    .ui
                    .hints_selected_center()
                    .unwrap_or_else(|| self.ui.cursor_browser_rel());
                self.browser.scroll(0.0, dy, x, y);
                self.ui.hints_mark_stale();
            }
            return;
        }

        if aim != (0.0, 0.0) {
            self.ui.move_cursor(
                aim.0 * cursor_speed * dt,
                aim.1 * cursor_speed * dt,
                &self.window,
            );
            // Only hover the page while the cursor is over it; over the toolbar
            // there's nothing in Servo to point at.
            if self.ui.cursor_over_browser() {
                let (x, y) = self.ui.cursor_browser_rel();
                self.browser
                    .handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
            }
        }

        if scroll != 0.0 && self.ui.cursor_over_browser() {
            // Stick down (+1) reveals lower content (positive Servo dy).
            let dy = scroll * scroll_speed * dt;
            let (x, y) = self.ui.cursor_browser_rel();
            self.browser.scroll(0.0, dy, x, y);
        }
    }

    /// Auto-repeat gate for held-stick overlay navigation: latches the direction
    /// and paces repeats, returning `true` on the frames a step should fire.
    fn nav_repeat(&mut self, dir: (i32, i32), now: Instant) -> bool {
        let cfg = &self.config.gamepad;
        if dir != self.osk_nav_dir {
            self.osk_nav_dir = dir;
            if dir != (0, 0) {
                self.osk_nav_next = now + Duration::from_millis(cfg.osk_nav_initial_delay_ms);
                return true;
            }
            return false;
        }
        if dir != (0, 0) && now >= self.osk_nav_next {
            self.osk_nav_next = now + Duration::from_millis(cfg.osk_nav_repeat_ms);
            return true;
        }
        false
    }
}

/// Reduce a stick vector to a single discrete grid step along its dominant axis,
/// or `(0, 0)` when the stick is within the navigation dead zone (`threshold`).
fn osk_nav_dir(v: (f32, f32), threshold: f32) -> (i32, i32) {
    if v.0.abs().max(v.1.abs()) < threshold {
        (0, 0)
    } else if v.0.abs() >= v.1.abs() {
        (v.0.signum() as i32, 0)
    } else {
        (0, v.1.signum() as i32)
    }
}
