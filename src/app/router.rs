//! The central input router: decides what a contextual [`InputCommand`] does
//! given the current state. This is where the "keyboard open? Hub open? cursor
//! over the page or toolbar?" branches live — the gamepad itself stays
//! state-agnostic and only emits intents.

use super::{
    App, AppCommand, GameInputMapsAction, GameMapEditAction, HomeAction, InputCommand,
    PromptAction, QuickAccessAction, UpdateNoticeAction,
};
use crate::browser::BrowserCommand;
use crate::command::Analog;
use crate::event::game::input_map::ClickButton;
use crate::event::gamepad::cursor_curve;
use crate::event::nav_repeat::Pace;
use crate::overlay::hints;
use crate::overlay::osk::{OskCommand, PadInput};
use crate::ui::Focus;
use std::time::{Duration, Instant};

/// The button as SDL spells it, which is what the page is told.
fn sdl_button(button: ClickButton) -> sdl2::mouse::MouseButton {
    match button {
        ClickButton::Left => sdl2::mouse::MouseButton::Left,
        ClickButton::Right => sdl2::mouse::MouseButton::Right,
        ClickButton::Middle => sdl2::mouse::MouseButton::Middle,
    }
}

impl App {
    /// Route one contextual input intent against the current input owner — see
    /// [`Focus`] for the overlay precedence.
    pub(super) fn route_input(&mut self, command: &InputCommand, out: &mut Vec<AppCommand>) {
        let focus = self.ui.focus();
        // Hint mode reads each intent first; what it passes keeps the bare page's
        // meaning, hence `Focus::Hints | Focus::Page` below.
        if focus == Focus::Hints && self.hints_input(command) {
            return;
        }
        match command {
            InputCommand::Confirm(pressed) => match focus {
                Focus::Osk => {
                    if *pressed {
                        self.osk_input(PadInput::A, out);
                    }
                }
                Focus::Prompt => {
                    if *pressed {
                        out.push(AppCommand::Prompt(PromptAction::Activate));
                    }
                }
                Focus::UpdateNotice => {
                    if *pressed {
                        out.push(AppCommand::UpdateNotice(UpdateNoticeAction::Activate));
                    }
                }
                Focus::Hub => {
                    if *pressed {
                        self.hub_open_selected(out);
                    }
                }
                Focus::QuickAccess => {
                    if *pressed {
                        out.push(AppCommand::QuickAccess(QuickAccessAction::Activate));
                    }
                }
                // A opens a map, or takes the row it is on.
                Focus::GameInputMaps => {
                    if *pressed {
                        out.push(AppCommand::GameInputMaps(GameInputMapsAction::Activate));
                    }
                }
                // A opens the focused row's list, then takes what it lands on.
                Focus::GameMapEdit => {
                    if *pressed {
                        out.push(AppCommand::GameMapEdit(GameMapEditAction::Activate));
                    }
                }
                // The settings overlay: A toggles / cycles / steps the focused
                // field, or opens the OSK on a text row (see `settings_confirm`).
                Focus::Settings => {
                    if *pressed {
                        self.settings_confirm(out);
                    }
                }
                // The start page: A opens the OSK to type into its search field,
                // or opens the focused bookmark (see [`App::home_confirm`]).
                Focus::Home => {
                    if *pressed {
                        self.home_confirm(out);
                    }
                }
                Focus::Hints | Focus::Page => self.primary_action(*pressed),
            },
            InputCommand::Cancel => match focus {
                Focus::Osk => {
                    self.osk_input(PadInput::B, out);
                }
                Focus::Prompt => out.push(AppCommand::Prompt(PromptAction::Cancel)),
                Focus::UpdateNotice => {
                    out.push(AppCommand::UpdateNotice(UpdateNoticeAction::Cancel))
                }
                Focus::Hub => self.ui.hub.close(),
                // B resumes the game; leaving Game Mode is a row of its own.
                Focus::QuickAccess => self.ui.quick_access.close(),
                // B backs out one screen; the list hands back to what opened it.
                Focus::GameInputMaps => {
                    out.push(AppCommand::GameInputMaps(GameInputMapsAction::Close))
                }
                // B saves what changed and goes back to the map it edited.
                Focus::GameMapEdit => out.push(AppCommand::GameMapEdit(GameMapEditAction::Close)),
                // B leaves a door's screen, else saves the draft and closes.
                Focus::Settings => {
                    if !self.ui.settings.close_door() {
                        self.settings_close(out);
                    }
                }
                // B on the start page goes back like a normal page — except that
                // it is also the only way out of a page holding fullscreen.
                Focus::Hints | Focus::Home | Focus::Page => {
                    let command = match self.browser.is_fullscreen() {
                        true => BrowserCommand::ExitFullscreen,
                        false => BrowserCommand::Back,
                    };
                    self.browser.execute_command(&command, &self.config);
                }
            },
            InputCommand::ToggleOsk => match focus {
                // X deletes the highlighted entry (closes a tab in the Tabs section).
                Focus::Hub => self.delete_hub_selection(),
                // X is unused in settings (rows edit with A and Left/Right) and
                // on Game Mode's screens (the keyboard has a row of its own).
                Focus::Settings | Focus::UpdateNotice => {}
                Focus::GameInputMaps => {}
                // X sets a site's own value back to the global one, or closes a tab.
                Focus::QuickAccess => out.push(AppCommand::QuickAccess(QuickAccessAction::Remove)),
                // X unbinds the focused source, which is what takes its row away.
                Focus::GameMapEdit => out.push(AppCommand::GameMapEdit(GameMapEditAction::Remove)),
                Focus::Osk => {
                    self.osk_input(PadInput::X, out);
                }
                Focus::Hints | Focus::Prompt | Focus::Home | Focus::Page => {
                    // The keyboard takes over the stick and A — leave hint mode.
                    self.ui.hints.hide();
                    // On a start-page tile X opens its options instead.
                    let tile = self.ui.home.tile().filter(|_| focus == Focus::Home);
                    if tile.is_some_and(|tile| self.open_options(tile)) {
                        return;
                    }
                    // On the start page, X types into the search field, or into
                    // the address bar when the page shows none.
                    if focus == Focus::Home {
                        match self.config.home.search.shown() {
                            true => self.ui.home.focus_search(),
                            false => self.ui.focus_address_bar(),
                        }
                    }
                    self.ui.osk(OskCommand::Show, &self.browser, out);
                }
            },
            // Tab switching is parked while a modal prompt is up — it belongs
            // to the page that opened it.
            InputCommand::CycleTab(delta) => {
                if !self.ui.prompt.visible() && !focus.takes_over() {
                    self.browser.cycle_tab(*delta);
                }
            }
            // One overlay-navigation step: whichever overlay is open owns it,
            // and with none open the handler forwards the arrows to the page.
            InputCommand::Nav(dx, dy) => match focus {
                Focus::Osk => {
                    self.osk_input(PadInput::Nav(*dx, *dy), out);
                }
                Focus::Prompt => self.ui.prompt.move_sel(*dx, *dy),
                Focus::UpdateNotice => {
                    let state = self.ui.update.snapshot();
                    self.ui.update_notice.move_sel(*dx, &state);
                }
                Focus::Hub => self.ui.hub.nav(*dx, *dy),
                Focus::QuickAccess => {
                    if *dy != 0 {
                        self.ui.quick_access.move_sel(*dy);
                    } else if *dx != 0 {
                        out.push(AppCommand::QuickAccess(QuickAccessAction::Adjust(*dx)));
                    }
                }
                Focus::GameInputMaps => self.ui.input_maps.move_sel(*dy),
                Focus::GameMapEdit => {
                    if *dy != 0 {
                        self.ui.map_edit.move_sel(*dy);
                    } else if *dx != 0 {
                        out.push(AppCommand::GameMapEdit(GameMapEditAction::Adjust(*dx)));
                    }
                }
                // Up/Down moves between rows, Left/Right adjusts the focused value.
                Focus::Settings => {
                    if *dy != 0 {
                        self.ui.settings_move(*dy);
                    } else if *dx != 0 {
                        self.ui.settings.adjust(*dx);
                    }
                }
                Focus::Home => self.ui.home_move(*dx, *dy),
                Focus::Hints | Focus::Page => {}
            },
            // Combo symbols and typed letters: hint mode's alone. Elsewhere the
            // D-pad already moves via the aim vector.
            InputCommand::DpadPress(..) | InputCommand::HintKey(_) => {}
            // Y / L3: a bookmark toggle, a space over the keyboard, or link
            // hints on the page.
            InputCommand::Hints => match focus {
                Focus::Hub => self.hub_y_action(),
                Focus::Osk => {
                    self.osk_input(PadInput::Y, out);
                }
                // Y on the start page cycles the grid's sort.
                Focus::Home => self.home_action(&HomeAction::StepSort(1), out),
                Focus::Prompt | Focus::Settings => {}
                Focus::UpdateNotice => {}
                Focus::QuickAccess | Focus::GameInputMaps | Focus::GameMapEdit => {}
                Focus::Hints | Focus::Page => {
                    self.ui.hints_begin_collect();
                    self.browser.collect_hints();
                }
            },
            InputCommand::Shoulder(delta) => {
                // The keyboard may claim them first.
                if self.osk_input(PadInput::Shoulder(*delta), out) {
                    return;
                }
                match focus {
                    Focus::Hub => self.ui.hub.switch_section(*delta),
                    // L1/R1 switch the settings section (Left/Right edits values).
                    Focus::Settings => self.ui.settings.switch_section(*delta),
                    // No sections to switch here — and page navigation under one of
                    // Game Mode's screens would leave the game.
                    Focus::QuickAccess | Focus::GameInputMaps | Focus::GameMapEdit => {}
                    Focus::UpdateNotice => {}
                    // On the start page they cycle the grid's filter.
                    Focus::Home => self.home_action(&HomeAction::StepFilter(*delta), out),
                    Focus::Osk | Focus::Prompt | Focus::Hints | Focus::Page => {
                        // Page navigation is parked while the modal prompt is up (it
                        // may sit under the keyboard), like tab switching.
                        if !self.ui.prompt.visible() {
                            let cmd = if *delta < 0 {
                                BrowserCommand::Back
                            } else {
                                BrowserCommand::Forward
                            };
                            self.browser.execute_command(&cmd, &self.config);
                        }
                    }
                }
            }
            // Only while the keyboard is up; both edges go, for the held
            // modifiers. Elsewhere the triggers are ordinary bindable pads.
            InputCommand::Trigger { right, pressed } => {
                let input = match right {
                    true => PadInput::RightTrigger(*pressed),
                    false => PadInput::LeftTrigger(*pressed),
                };
                self.osk_input(input, out);
            }
            // The stock zoom pads are the keyboard's Shift and Enter.
            InputCommand::Zoom(delta) => {
                if focus != Focus::Osk && !focus.takes_over() {
                    let cmd = BrowserCommand::Zoom(*delta);
                    self.browser.execute_command(&cmd, &self.config);
                }
            }
            // Dedicated keyboard keys act only while the keyboard is open. The one
            // exception is Y (Space): outside the keyboard it reloads the page.
            InputCommand::Osk(cmd) => {
                if focus == Focus::Osk {
                    self.ui.osk(*cmd, &self.browser, out);
                } else if matches!(cmd, OskCommand::Space) && !focus.takes_over() {
                    self.browser
                        .execute_command(&BrowserCommand::Reload, &self.config);
                }
            }
            // A game's own click with a button Confirm cannot carry: the page
            // only, since the chrome has no second button to press.
            InputCommand::Click { button, pressed } => {
                if self.ui.cursor_over_browser() {
                    let (x, y) = self.ui.cursor_browser_rel();
                    self.browser.mouse_move(x, y);
                    self.browser
                        .mouse_button(sdl_button(*button), x, y, *pressed);
                }
            }
            InputCommand::OskButton(input) => {
                self.osk_input(*input, out);
            }
            InputCommand::Analog(analog) => self.route_analog(*analog, out),
        }
    }

    /// Offer the keyboard a pad input while it has focus; whether it took it.
    pub(super) fn osk_input(&mut self, input: PadInput, out: &mut Vec<AppCommand>) -> bool {
        self.ui.focus() == Focus::Osk && self.ui.osk_input(input, &self.browser, out)
    }

    /// Offer hint mode an input intent and carry out what it asks; whether it
    /// took it.
    fn hints_input(&mut self, command: &InputCommand) -> bool {
        let cx = hints::Context {
            hold: Duration::from_millis(self.config.controls.hold_ms),
            now: Instant::now(),
            area: (self.ui.browser_area_width(), self.ui.browser_area_height()),
            cursor: self.ui.cursor_browser_rel(),
        };
        match self.ui.hints.read(command, &cx) {
            hints::Reading::Pass => return false,
            hints::Reading::Taken => {}
            hints::Reading::Click((x, y)) => self.click_page(x, y),
            hints::Reading::OpenInBackground(url) => self.browser.open_tab_background(&url),
            hints::Reading::Scroll { dy, at: (x, y) } => {
                self.ui.scroll_page(&self.browser, 0.0, dy, x, y);
            }
        }
        true
    }

    /// Click the page at a browser-relative point: a synthetic mouse move + press +
    /// release, so JS click handlers fire like for a real click.
    fn click_page(&mut self, x: f32, y: f32) {
        self.browser.mouse_move(x, y);
        for pressed in [true, false] {
            self.browser
                .mouse_button(sdl2::mouse::MouseButton::Left, x, y, pressed);
        }
    }

    /// The A button with no overlay up: click the page in Servo or the egui
    /// toolbar — whichever the cursor is currently over.
    fn primary_action(&mut self, pressed: bool) {
        if self.ui.cursor_over_browser() {
            let (x, y) = self.ui.cursor_browser_rel();
            self.browser.mouse_move(x, y);
            self.browser
                .mouse_button(sdl2::mouse::MouseButton::Left, x, y, pressed);
        } else {
            self.ui.click_ui(pressed, &mut self.window);
        }
    }

    /// Apply per-frame analog state: grid navigation while the keyboard is open,
    /// otherwise cursor movement and page scroll. The roles of the D-pad and the
    /// sticks apply to the bare page only; an overlay gets the raw aim.
    fn route_analog(&mut self, analog: Analog, out: &mut Vec<AppCommand>) {
        let Analog {
            aim,
            stick,
            cursor,
            cursor_uncurved,
            scroll,
            right,
        } = analog;
        self.ui.set_edge_scroll((0, 0));
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;
        // The loop blocks on input while idle, so the first frame after a press
        // would integrate the whole gap; an over-long frame is a fresh start.
        let dt = if dt > 0.1 { 0.0 } else { dt.min(0.05) };
        // Scalar copies: the config holds non-Copy data (the bindings map), so
        // it can't be borrowed across the `&mut self` calls below.
        let cfg = &self.config.controls;
        let (cursor_speed, scroll_speed, nav_threshold, edge_scroll) = (
            cfg.cursor_speed,
            cfg.scroll_speed,
            cfg.osk_nav_threshold,
            cfg.edge_scroll,
        );
        let (deadzone, curve) = (cfg.deadzone, cfg.cursor_curve);
        let pace = Pace {
            threshold: nav_threshold,
            initial: Duration::from_millis(cfg.osk_nav_initial_delay_ms),
            repeat: Duration::from_millis(cfg.osk_nav_repeat_ms),
        };

        // The right stick carries the keyboard at cursor speed, and scrolls the
        // page once it is pinned against an edge.
        if right != (0.0, 0.0) {
            let step = cursor_speed * dt;
            let moved = self.osk_input(PadInput::Move(right.0 * step, right.1 * step), out);
            if moved && edge_scroll {
                self.edge_scroll(right, self.ui.osk_clipped(), scroll_speed * dt);
            }
        }
        // The keyboard may claim the stick.
        if self.osk_input(PadInput::Stick(stick, nav_threshold), out) {
            return;
        }

        // Over an overlay the stick becomes the same discrete `Nav` steps the
        // keyboard arrows emit, so both devices share one execution path.
        if self.ui.focus() != Focus::Page {
            // Hint mode with combos on hops on the stick alone: the D-pad is
            // reserved for combo symbols, so it must not also drive a hop.
            let nav_vec = if self.ui.focus() == Focus::Hints && self.ui.hints.badges {
                stick
            } else {
                aim
            };
            if let Some((dx, dy)) = self.nav_repeat.step(nav_vec, &pace, now) {
                out.push(AppCommand::Input(InputCommand::Nav(dx, dy)));
            }
            // In hint mode the right stick still scrolls the page; the scroll
            // itself schedules the re-collect its movement calls for.
            if self.ui.hints.visible && scroll != (0.0, 0.0) {
                let (dx, dy) = (scroll.0 * scroll_speed * dt, scroll.1 * scroll_speed * dt);
                let (x, y) = self.ui.hints.anchor(self.ui.cursor_browser_rel());
                self.ui.scroll_page(&self.browser, dx, dy, x, y);
            }
            return;
        }

        let page_aim = (
            (cursor_uncurved.0 + cursor.0).clamp(-1.0, 1.0),
            (cursor_uncurved.1 + cursor.1).clamp(-1.0, 1.0),
        );
        if page_aim != (0.0, 0.0) {
            // Only the stick's share of the aim takes the curve: the D-pad is
            // already full deflection, and Game Mode curves its own sticks.
            let curved = cursor_curve(cursor, deadzone, curve);
            let aim = (
                (cursor_uncurved.0 + curved.0).clamp(-1.0, 1.0),
                (cursor_uncurved.1 + curved.1).clamp(-1.0, 1.0),
            );
            let clipped = self.ui.move_cursor(
                aim.0 * cursor_speed * dt,
                aim.1 * cursor_speed * dt,
                &self.window,
            );
            if edge_scroll && !self.browser.in_game_mode() {
                self.edge_scroll(aim, clipped, scroll_speed * dt);
            }
            // Only hover the page while the cursor is over it; over the toolbar
            // there's nothing in Servo to point at.
            if self.ui.cursor_over_browser() {
                let (x, y) = self.ui.cursor_browser_rel();
                self.browser.mouse_move(x, y);
            }
        }

        if scroll != (0.0, 0.0) && self.ui.cursor_over_browser() {
            // Stick down (+1) reveals lower content (positive Servo dy).
            let (dx, dy) = (scroll.0 * scroll_speed * dt, scroll.1 * scroll_speed * dt);
            let (x, y) = self.ui.cursor_browser_rel();
            self.ui.scroll_page(&self.browser, dx, dy, x, y);
        }
    }

    /// Scroll the page along each axis where the window edge clipped the cursor,
    /// at the scroll stick's rate (`step` is px per unit of deflection this frame).
    fn edge_scroll(&mut self, aim: (f32, f32), clipped: (f32, f32), step: f32) {
        // Clipped against the push, not by a window that shrank under the cursor.
        let dir = |aim: f32, clip: f32| {
            if aim * clip > 0.0 {
                aim.signum() as i8
            } else {
                0
            }
        };
        let dir = (dir(aim.0, clipped.0), dir(aim.1, clipped.1));
        let (width, height) = (self.ui.browser_area_width(), self.ui.browser_area_height());
        if dir == (0, 0) || width <= 0.0 || height <= 0.0 {
            return;
        }
        let along = |dir: i8, aim: f32| if dir != 0 { aim * step } else { 0.0 };
        let (dx, dy) = (along(dir.0, aim.0), along(dir.1, aim.1));
        // Hit-tested at mid-viewport: an edge usually sits in a sticky header or bar.
        self.ui
            .scroll_page(&self.browser, dx, dy, width / 2.0, height / 2.0);
        self.ui.set_edge_scroll(dir);
    }
}
