//! The central input router: decides what a contextual [`InputCommand`] does
//! given the current state. This is where the "keyboard open? menu open? cursor
//! over the page or toolbar?" branches live — the gamepad itself stays
//! state-agnostic and only emits intents.

use super::{App, AppCommand, InputCommand, PromptAction};
use crate::browser::BrowserCommand;
use crate::event::sdl2_servo::{into_mouse_button_event, into_mouse_move_event};
use crate::overlay::osk::OskCommand;
use crate::ui::Focus;
use std::time::{Duration, Instant};

impl App {
    /// Route one contextual input intent against the current input owner —
    /// see [`Focus`] for the overlay precedence (the on-screen keyboard stays
    /// above the modal prompt, since it's how a gamepad types into a
    /// `prompt()` field).
    pub(super) fn route_input(&mut self, command: &InputCommand, out: &mut Vec<AppCommand>) {
        let focus = self.ui.focus();
        match command {
            InputCommand::Confirm(pressed) => match focus {
                Focus::Osk => {
                    if *pressed {
                        self.ui.osk(OskCommand::Activate, &self.browser, out);
                    }
                }
                Focus::Prompt => {
                    if *pressed {
                        out.push(AppCommand::Prompt(PromptAction::Activate));
                    }
                }
                Focus::Menu => {
                    if *pressed {
                        self.menu_open_selected();
                    }
                }
                // The settings overlay: A toggles / cycles / steps the focused
                // field, or opens the OSK on a text row (see `settings_confirm`).
                Focus::Settings => {
                    if *pressed {
                        self.settings_confirm(out);
                    }
                }
                Focus::Hints => {
                    // Tap vs hold on the selected hint: the press just starts the
                    // clock (so the click lands on release, where the duration is
                    // known); a hold past the gesture threshold opens the hint's
                    // link in a background tab instead, a tap clicks it as before.
                    if *pressed {
                        self.hint_press_at = Some(Instant::now());
                    } else {
                        let hold = Duration::from_millis(self.config.gamepad.hold_ms);
                        let held_long = self
                            .hint_press_at
                            .take()
                            .is_some_and(|t| t.elapsed() >= hold);
                        match self.ui.hints_selected_url().filter(|_| held_long) {
                            Some(url) => {
                                self.ui.hints_hide();
                                self.browser.open_tab_background(&url);
                            }
                            None => self.activate_hint(),
                        }
                    }
                }
                // The start page: A opens the OSK to type into its search field,
                // or opens the focused speed-dial tile (see [`App::home_confirm`]).
                Focus::Home => {
                    if *pressed {
                        self.home_confirm(out);
                    }
                }
                // The speed-dial editor: A types into the field / pins via Add
                // (tiles are edit-only — see [`App::dial_edit_confirm`]).
                Focus::DialEdit => {
                    if *pressed {
                        self.dial_edit_confirm(out);
                    }
                }
                Focus::Page => self.primary_action(*pressed),
            },
            InputCommand::Cancel => match focus {
                Focus::Osk => self.ui.osk(OskCommand::Hide, &self.browser, out),
                Focus::Prompt => out.push(AppCommand::Prompt(PromptAction::Cancel)),
                Focus::Menu => self.ui.menu_close(),
                // B saves the draft and closes (same as the ✖ button).
                Focus::Settings => self.settings_close(),
                Focus::Hints => self.ui.hints_hide(),
                // B in the editor returns to the start page.
                Focus::DialEdit => self.ui.close_pins_editor(),
                // B on the start page goes back like a normal page.
                Focus::Home | Focus::Page => self
                    .browser
                    .execute_command(&BrowserCommand::Back, &self.config.browser),
            },
            InputCommand::ToggleOsk => {
                if focus == Focus::Menu {
                    // X deletes the highlighted entry (closes a tab in the Tabs section).
                    self.delete_menu_selection();
                } else if focus == Focus::DialEdit {
                    // X deletes the focused pin tile (no-op on the field or the
                    // trailing ⚙ settings toggle, which pins/unpins with A).
                    self.ui.dial_edit_remove_selected();
                } else if focus == Focus::Settings {
                    // X is unused in settings (rows edit with A and ◀▶).
                } else {
                    // The keyboard takes over the stick and A — leave hint mode.
                    self.ui.hints_hide();
                    // On the start page, X types into the search field — focus it
                    // so a tile selection doesn't swallow the typed text.
                    if focus == Focus::Home {
                        self.ui.home_focus_search();
                    }
                    let cmd = if focus == Focus::Osk {
                        OskCommand::Backspace
                    } else {
                        OskCommand::Show
                    };
                    self.ui.osk(cmd, &self.browser, out);
                }
            }
            // Tab switching is parked while a modal prompt is up — it belongs
            // to the page that opened it.
            InputCommand::CycleTab(delta) => {
                if !self.ui.prompt.visible() && focus != Focus::Settings {
                    self.browser.cycle_tab(*delta);
                }
            }
            // One overlay-navigation step (keyboard arrows / nav_* bindings, or
            // the stick shaped by `route_analog`): whichever overlay is open
            // owns it; with none open it's a no-op (the event handler forwards
            // unconsumed arrows to the page instead).
            InputCommand::Nav(dx, dy) => match focus {
                Focus::Osk => self.ui.osk(OskCommand::Move(*dx, *dy), &self.browser, out),
                Focus::Prompt => self.ui.prompt.move_sel(*dx, *dy),
                Focus::Menu => {
                    if *dx != 0 {
                        self.ui.menu_switch(*dx);
                    } else if *dy != 0 {
                        self.ui.menu_move(*dy);
                    }
                }
                // ▲▼ moves between rows, ◀▶ adjusts the focused value.
                Focus::Settings => {
                    if *dy != 0 {
                        self.ui.settings_move(*dy);
                    } else if *dx != 0 {
                        self.ui.settings_adjust(*dx);
                    }
                }
                Focus::Hints => self.ui.hints_move((*dx, *dy)),
                Focus::Home => self.ui.home_move(*dx, *dy),
                Focus::DialEdit => self.ui.dial_edit_move(*dx, *dy),
                Focus::Page => {}
            },
            // Y / L3: contextually a pin/bookmark toggle or link-hint navigation.
            // In the menu it depends on the section — Bookmarks pins (or unpins)
            // the selected entry to the speed dial, while History and Tabs toggle
            // a bookmark on the selected entry / tab. With the keyboard open it
            // types a space (its dedicated OSK shortcut — see [`crate::overlay::osk`]).
            // On the bare page it toggles link hints (collection is asynchronous —
            // badges appear once the page reports its elements). The start page
            // ignores it: pins are managed from the speed-dial editor, not unpinned
            // by a stray Y on a tile.
            InputCommand::Hints => match focus {
                Focus::Menu => self.menu_y_action(),
                Focus::Osk => self.ui.osk(OskCommand::Space, &self.browser, out),
                Focus::Home | Focus::Prompt | Focus::DialEdit | Focus::Settings => {}
                Focus::Hints => self.ui.hints_hide(),
                Focus::Page => {
                    self.ui.hints_begin_collect();
                    self.browser.collect_hints();
                }
            },
            InputCommand::Shoulder(delta) => match focus {
                Focus::Menu => self.ui.menu_switch(*delta),
                // L1/R1 switch the settings section (◀▶ is taken by value editing).
                Focus::Settings => self.ui.settings_switch(*delta),
                // Page navigation is parked while the modal prompt is up (it
                // may sit under the keyboard), like tab switching.
                _ if self.ui.prompt.visible() => {}
                _ => {
                    let cmd = if *delta < 0 {
                        BrowserCommand::Back
                    } else {
                        BrowserCommand::Foward
                    };
                    self.browser.execute_command(&cmd, &self.config.browser);
                }
            },
            InputCommand::Trigger { right, pressed } => {
                if focus == Focus::Osk {
                    // Keyboard: L2 is a held Shift, R2 is Enter on the press edge.
                    if *right {
                        if *pressed {
                            self.ui.osk(OskCommand::Enter, &self.browser, out);
                        }
                    } else {
                        self.ui.osk(OskCommand::Shift(*pressed), &self.browser, out);
                    }
                } else if *pressed && !self.ui.prompt.visible() && focus != Focus::Settings {
                    // Quick tab switch: L2 previous, R2 next (wraps).
                    self.browser.cycle_tab(if *right { 1 } else { -1 });
                }
            }
            // Dedicated keyboard keys act only while the keyboard is open. The one
            // exception is Y (Space): outside the keyboard it reloads the page.
            InputCommand::Osk(cmd) => {
                if focus == Focus::Osk {
                    self.ui.osk(*cmd, &self.browser, out);
                } else if matches!(cmd, OskCommand::Space) && focus != Focus::Settings {
                    self.browser
                        .execute_command(&BrowserCommand::Reload, &self.config.browser);
                }
            }
            InputCommand::Analog {
                aim,
                scroll,
                scroll_mode,
            } => self.route_analog(*aim, *scroll, *scroll_mode, out),
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

    /// The A button with no overlay up: click the page in Servo or the egui
    /// toolbar — whichever the cursor is currently over.
    fn primary_action(&mut self, pressed: bool) {
        if self.ui.cursor_over_browser() {
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
    /// `scroll_mode` only changes the bare-page meaning of the aim vector
    /// (scroll instead of cursor) — overlay navigation always gets the raw aim.
    fn route_analog(
        &mut self,
        aim: (f32, f32),
        scroll: f32,
        scroll_mode: bool,
        out: &mut Vec<AppCommand>,
    ) {
        // Keep the UI's scroll-mode indicator in sync (drawn in place of the
        // cursor while the mode is latched).
        self.ui.set_scroll_mode(scroll_mode);
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

        // Overlays (menu / OSK / hints / prompt): the stick becomes discrete
        // navigation, shaped (dead zone + auto-repeat) into the same `Nav` steps
        // the keyboard arrows emit — one execution path for both devices. The menu
        // takes the dominant axis only, so a diagonal nudge does just one thing.
        if self.ui.focus() != Focus::Page {
            let dir = osk_nav_dir(aim, nav_threshold);
            if self.nav_repeat(dir, now) && dir != (0, 0) {
                out.push(AppCommand::Input(InputCommand::Nav(dir.0, dir.1)));
            }
            // In hint mode the right stick still scrolls the page (the badges
            // go stale as it moves — schedule a re-collect).
            if self.ui.hints_visible() && scroll != 0.0 {
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

        // Scroll mode: the aim vector scrolls the page (combined with the right
        // stick) and the cursor stays parked.
        if scroll_mode {
            let dy = (scroll + aim.1).clamp(-1.0, 1.0) * scroll_speed * dt;
            if dy != 0.0 {
                // The parked cursor may sit over the toolbar; scroll the page
                // from its top edge in that case.
                let (x, y) = self.ui.cursor_browser_rel();
                self.browser.scroll(0.0, dy, x, y.max(1.0));
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
