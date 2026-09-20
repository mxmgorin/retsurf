//! The central input router: decides what a contextual [`InputCommand`] does
//! given the current state. This is where the "keyboard open? menu open? cursor
//! over the page or toolbar?" branches live — the gamepad itself stays
//! state-agnostic and only emits intents.

use super::{
    App, AppCommand, GameInputMapsAction, GameMapEditAction, GameMenuAction, InputCommand,
    PromptAction,
};
use crate::browser::BrowserCommand;
use crate::event::game::input_map::ClickButton;
use crate::overlay::hints::{HintInput, Sym};
use crate::overlay::osk::OskCommand;
use crate::ui::Focus;
use std::time::{Duration, Instant};

/// How much of the viewport a hint-mode edge auto-scroll covers — a chunk shy of
/// a full screen, so a strip of the old hints stays on-screen for continuity.
const HINT_EDGE_SCROLL_FRACTION: f32 = 0.8;

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
                Focus::GameMenu => {
                    if *pressed {
                        out.push(AppCommand::GameMenu(GameMenuAction::Activate));
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
                Focus::Hints => {
                    // Tap vs hold on the selected hint: the press starts the
                    // clock, and a hold opens the link in a background tab.
                    if *pressed {
                        self.hint_press_at = Some(Instant::now());
                    } else {
                        let hold = Duration::from_millis(self.config.input.hold_ms);
                        let held_long = self
                            .hint_press_at
                            .take()
                            .is_some_and(|t| t.elapsed() >= hold);
                        let url = self.ui.hints.selected_url().filter(|_| held_long);
                        match url.map(str::to_owned) {
                            Some(url) => {
                                self.ui.hints.hide();
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
                Focus::Menu => self.ui.menu.close(),
                // B resumes the game; leaving Game Mode is a row of its own.
                Focus::GameMenu => self.ui.game_menu.close(),
                // B backs out one screen; the list hands the menu back.
                Focus::GameInputMaps => {
                    out.push(AppCommand::GameInputMaps(GameInputMapsAction::Close))
                }
                // B saves what changed and goes back to the map it edited.
                Focus::GameMapEdit => out.push(AppCommand::GameMapEdit(GameMapEditAction::Close)),
                // B saves the draft and closes (same as the close button).
                Focus::Settings => self.settings_close(out),
                // B drops a half-typed combo first, then exits hint mode.
                Focus::Hints => {
                    if self.ui.hints.has_typed() {
                        self.ui.hints.clear_typed();
                    } else {
                        self.ui.hints.hide();
                    }
                }
                // B in the editor returns to the start page.
                Focus::DialEdit => self.ui.dial_edit.close(),
                // B on the start page goes back like a normal page — except that
                // it is also the only way out of a page holding fullscreen.
                Focus::Home | Focus::Page => {
                    let command = match self.browser.is_fullscreen() {
                        true => BrowserCommand::ExitFullscreen,
                        false => BrowserCommand::Back,
                    };
                    self.browser.execute_command(&command, &self.config.browser);
                }
            },
            InputCommand::ToggleOsk => match focus {
                // X deletes the highlighted entry (closes a tab in the Tabs section).
                Focus::Menu => self.delete_menu_selection(),
                // X deletes the focused pin tile (no-op on the field or the
                // trailing "Pin settings" tile, which adds with A).
                Focus::DialEdit => self.ui.dial_edit_remove_selected(),
                // X is unused in settings (rows edit with A and Left/Right) and
                // on Game Mode's menu screens (the keyboard has a row of its own).
                Focus::Settings => {}
                Focus::GameMenu | Focus::GameInputMaps => {}
                // X unbinds the focused source, which is what takes its row away.
                Focus::GameMapEdit => out.push(AppCommand::GameMapEdit(GameMapEditAction::Remove)),
                // In hint mode X is a combo symbol, not the OSK toggle (unless
                // combos are disabled, when it falls through to the OSK below).
                Focus::Hints if self.config.input.hint_badges => self.hint_sym(Sym::X),
                Focus::Hints | Focus::Osk | Focus::Prompt | Focus::Home | Focus::Page => {
                    // The keyboard takes over the stick and A — leave hint mode.
                    self.ui.hints.hide();
                    // On the start page, X types into the search field — focus it
                    // so a tile selection doesn't swallow the typed text.
                    if focus == Focus::Home {
                        self.ui.home.focus_search();
                    }
                    let cmd = if focus == Focus::Osk {
                        OskCommand::Backspace
                    } else {
                        OskCommand::Show
                    };
                    self.ui.osk(cmd, &self.browser, out);
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
                Focus::Osk => self.ui.osk(OskCommand::Move(*dx, *dy), &self.browser, out),
                Focus::Prompt => self.ui.prompt.move_sel(*dx, *dy),
                Focus::Menu => {
                    if *dx != 0 {
                        self.ui.menu.switch_section(*dx);
                    } else if *dy != 0 {
                        self.ui.menu.move_sel(*dy);
                    }
                }
                Focus::GameMenu => self.ui.game_menu.move_sel(*dy),
                Focus::GameInputMaps => self.ui.input_maps.move_sel(*dy),
                Focus::GameMapEdit => self.ui.map_edit.move_sel(*dy),
                // Up/Down moves between rows, Left/Right adjusts the focused value.
                Focus::Settings => {
                    if *dy != 0 {
                        self.ui.settings_move(*dy);
                    } else if *dx != 0 {
                        self.ui.settings.adjust(*dx);
                    }
                }
                Focus::Hints => self.hints_nav(*dx, *dy),
                Focus::Home => self.ui.home_move(*dx, *dy),
                Focus::DialEdit => self.ui.dial_edit_move(*dx, *dy),
                Focus::Page => {}
            },
            // Discrete D-pad press: in hint mode it types a combo symbol;
            // everywhere else the D-pad already moves via the aim vector.
            InputCommand::DpadPress(dx, dy) => {
                if focus == Focus::Hints && self.config.input.hint_badges {
                    if let Some(sym) = dpad_sym(*dx, *dy) {
                        self.hint_sym(sym);
                    }
                }
            }
            // A typed letter for a keyboard-driven hint round (the keyboard handler
            // only emits it in that mode); resolve it like a gamepad combo symbol.
            InputCommand::HintKey(c) => {
                if focus == Focus::Hints && self.config.input.hint_badges {
                    self.hint_key(*c);
                }
            }
            // Y / L3: a pin/bookmark toggle, a space over the keyboard, or link
            // hints on the page. Unpinning is the dial editor's job, not a press.
            InputCommand::Hints => match focus {
                Focus::Menu => self.menu_y_action(),
                Focus::Osk => self.ui.osk(OskCommand::Space, &self.browser, out),
                Focus::Home | Focus::Prompt | Focus::DialEdit | Focus::Settings => {}
                Focus::GameMenu | Focus::GameInputMaps | Focus::GameMapEdit => {}
                // In hint mode Y is a combo symbol (B exits instead); with combos
                // off it keeps its old meaning of hiding the hints.
                Focus::Hints if self.config.input.hint_badges => self.hint_sym(Sym::Y),
                Focus::Hints => self.ui.hints.hide(),
                Focus::Page => {
                    self.ui.hints_begin_collect();
                    self.browser.collect_hints();
                }
            },
            InputCommand::Shoulder(delta) => match focus {
                Focus::Menu => self.ui.menu.switch_section(*delta),
                // L1/R1 switch the settings section (Left/Right edits values).
                Focus::Settings => self.ui.settings.switch_section(*delta),
                // No sections to switch here — and page navigation under one of
                // Game Mode's screens would leave the game.
                Focus::GameMenu | Focus::GameInputMaps | Focus::GameMapEdit => {}
                // In the dial editor they reorder the focused pin (Left/Right
                // moves the selection there).
                Focus::DialEdit => self.ui.dial_edit_move_selected(*delta),
                // In hint mode L1/R1 are combo symbols; with combos off they fall
                // through to the page back/forward below.
                Focus::Hints if self.config.input.hint_badges => {
                    self.hint_sym(if *delta < 0 { Sym::L1 } else { Sym::R1 })
                }
                Focus::Osk | Focus::Prompt | Focus::Hints | Focus::Home | Focus::Page => {
                    // Page navigation is parked while the modal prompt is up (it
                    // may sit under the keyboard), like tab switching.
                    if !self.ui.prompt.visible() {
                        let cmd = if *delta < 0 {
                            BrowserCommand::Back
                        } else {
                            BrowserCommand::Forward
                        };
                        self.browser.execute_command(&cmd, &self.config.browser);
                    }
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
                } else if *pressed && !self.ui.prompt.visible() && !focus.takes_over() {
                    // Quick tab switch: L2 previous, R2 next (wraps).
                    self.browser.cycle_tab(if *right { 1 } else { -1 });
                }
            }
            // Dedicated keyboard keys act only while the keyboard is open. The one
            // exception is Y (Space): outside the keyboard it reloads the page.
            InputCommand::Osk(cmd) => {
                if focus == Focus::Osk {
                    self.ui.osk(*cmd, &self.browser, out);
                } else if matches!(cmd, OskCommand::Space) && !focus.takes_over() {
                    self.browser
                        .execute_command(&BrowserCommand::Reload, &self.config.browser);
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
            InputCommand::Analog {
                aim,
                stick,
                scroll,
                scroll_mode,
            } => self.route_analog(*aim, *stick, *scroll, *scroll_mode, out),
        }
    }

    /// Hint-mode directional input: hop the selection, or scroll a chunk and
    /// re-collect when it is already at a vertical edge. Horizontal edges stay
    /// put — pages rarely scroll sideways.
    fn hints_nav(&mut self, dx: i32, dy: i32) {
        if self.ui.hints.move_sel((dx, dy)) || dy == 0 {
            return;
        }
        let height = self.ui.browser_area_height();
        if height <= 0.0 {
            return;
        }
        // The scroll is hit-tested to pick its scroller: the hint's column, at
        // mid-viewport, because an edge hint often sits in a sticky header.
        let (sx, _) = self
            .ui
            .hints
            .selected_center()
            .unwrap_or_else(|| self.ui.cursor_browser_rel());
        let width = self.ui.browser_area_width();
        let sx = sx.clamp(1.0, (width - 1.0).max(1.0));
        let sy = height / 2.0;
        // dy > 0 = down: reveal lower content (positive Servo dy) and re-anchor
        // the selection to the bottom edge; up is the mirror.
        let chunk = dy as f32 * height * HINT_EDGE_SCROLL_FRACTION;
        self.ui.scroll_page(&self.browser, 0.0, chunk, sx, sy);
        let edge_y = if dy > 0 { height } else { 0.0 };
        self.ui.hints.mark_stale_at((sx, edge_y));
    }

    /// Feed a combo symbol to hint mode and, when it resolves to one hint, click
    /// it. A dead end clears the buffer inside `push_sym`, where the faded badges
    /// already show it went nowhere; a `Pending` combo waits for more.
    fn hint_sym(&mut self, sym: Sym) {
        if let HintInput::Activate(idx) = self.ui.hints_push_sym(sym) {
            self.ui.hints.select(idx);
            self.activate_hint();
        }
    }

    /// Feed a typed letter to a keyboard hint round; on a resolved code, click the
    /// matched hint (shares `hint_sym`'s activation path — see [`HintInput`]).
    fn hint_key(&mut self, c: char) {
        if let HintInput::Activate(idx) = self.ui.hints_push_key(c) {
            self.ui.hints.select(idx);
            self.activate_hint();
        }
    }

    /// Click the selected hint: a synthetic mouse move + press + release at its
    /// center (so JS click handlers fire like for a real click), then leave hint
    /// mode — the click usually navigates, invalidating the rects anyway.
    fn activate_hint(&mut self) {
        let Some((x, y)) = self.ui.hints.selected_center() else {
            self.ui.hints.hide();
            return;
        };
        self.ui.hints.hide();
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
    /// otherwise cursor movement and page scroll. `scroll_mode` changes only the
    /// bare-page meaning of the aim vector; an overlay gets the raw aim.
    fn route_analog(
        &mut self,
        aim: (f32, f32),
        stick: (f32, f32),
        scroll: (f32, f32),
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
        // would integrate the whole gap; an over-long frame is a fresh start.
        let dt = if dt > 0.1 { 0.0 } else { dt.min(0.05) };
        // Scalar copies: the config holds non-Copy data (the bindings map), so
        // it can't be borrowed across the `&mut self` calls below.
        let cfg = &self.config.input;
        let (cursor_speed, scroll_speed, nav_threshold, hint_badges) = (
            cfg.cursor_speed,
            cfg.scroll_speed,
            cfg.osk_nav_threshold,
            cfg.hint_badges,
        );

        // Over an overlay the stick becomes the same discrete `Nav` steps the
        // keyboard arrows emit, so both devices share one execution path.
        if self.ui.focus() != Focus::Page {
            // Hint mode with combos on hops on the stick alone: the D-pad is
            // reserved for combo symbols, so it must not also drive a hop.
            let nav_vec = if self.ui.focus() == Focus::Hints && hint_badges {
                stick
            } else {
                aim
            };
            let dir = osk_nav_dir(nav_vec, nav_threshold);
            if self.nav_repeat(dir, now) && dir != (0, 0) {
                out.push(AppCommand::Input(InputCommand::Nav(dir.0, dir.1)));
            }
            // In hint mode the right stick still scrolls the page (the badges
            // go stale as it moves — schedule a re-collect).
            if self.ui.hints.visible && scroll != (0.0, 0.0) {
                let (dx, dy) = (scroll.0 * scroll_speed * dt, scroll.1 * scroll_speed * dt);
                let (x, y) = self
                    .ui
                    .hints
                    .selected_center()
                    .unwrap_or_else(|| self.ui.cursor_browser_rel());
                self.ui.scroll_page(&self.browser, dx, dy, x, y);
                self.ui.hints.mark_stale();
            }
            return;
        }

        // Scroll mode: the aim vector scrolls the page (combined with the right
        // stick) and the cursor stays parked.
        if scroll_mode {
            let dx = scroll.0 * scroll_speed * dt;
            let dy = (scroll.1 + aim.1).clamp(-1.0, 1.0) * scroll_speed * dt;
            if (dx, dy) != (0.0, 0.0) {
                // The parked cursor may sit over the toolbar; scroll the page
                // from its top edge in that case.
                let (x, y) = self.ui.cursor_browser_rel();
                self.ui.scroll_page(&self.browser, dx, dy, x, y.max(1.0));
                // Keep the scroll-mode indicator alive while actively scrolling;
                // it lingers and auto-hides like the cursor once scrolling stops.
                self.ui.mark_cursor_active();
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

    /// Auto-repeat gate for held-stick overlay navigation: latches the direction
    /// and paces repeats, returning `true` on the frames a step should fire.
    fn nav_repeat(&mut self, dir: (i32, i32), now: Instant) -> bool {
        let cfg = &self.config.input;
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

/// Map a discrete D-pad press direction to its combo symbol (hint mode).
fn dpad_sym(dx: i32, dy: i32) -> Option<Sym> {
    Some(match (dx, dy) {
        (0, -1) => Sym::Up,
        (0, 1) => Sym::Down,
        (-1, 0) => Sym::Left,
        (1, 0) => Sym::Right,
        _ => return None,
    })
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
