use super::gamepad::Gamepad;
use super::keyboard::{KeyEvent, Keyboard};
use crate::{
    app::{AppCommand, SettingsAction},
    browser::AppBrowser,
    config::InputConfig,
    event::{user::handle_user, window::handle_window},
    platform::window::AppWindow,
    ui::AppUi,
};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

/// A bare modifier key (Ctrl/Shift/Alt/Gui on its own): during shortcut capture
/// these are ignored so the captured combo waits for the actual key.
fn is_modifier_key(kc: Keycode) -> bool {
    matches!(
        kc,
        Keycode::LCtrl
            | Keycode::RCtrl
            | Keycode::LShift
            | Keycode::RShift
            | Keycode::LAlt
            | Keycode::RAlt
            | Keycode::LGui
            | Keycode::RGui
    )
}

pub struct AppEventHandler {
    event_pump: sdl2::EventPump,
    game_controllers: Vec<sdl2::controller::GameController>,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
    /// Controller state machine: sticks/triggers, tap/hold/chord gestures.
    gamepad: Gamepad,
    /// Keyboard-side counterpart of [`Gamepad`]: shortcuts + overlay keys.
    keyboard: Keyboard,
    /// Single-finger touch gestures (drag→scroll, tap→click) over the web view.
    touch: super::touch::TouchState,
}

impl AppEventHandler {
    pub fn new(sdl: &sdl2::Sdl, gamepad_cfg: InputConfig) -> Result<Self, String> {
        let mut game_controllers = vec![];
        let game_controller_subsystem = sdl.game_controller()?;

        for id in 0..game_controller_subsystem.num_joysticks()? {
            if game_controller_subsystem.is_game_controller(id) {
                let controller = game_controller_subsystem.open(id).unwrap();
                game_controllers.push(controller);
            }
        }

        Ok(Self {
            event_pump: sdl.event_pump()?,
            game_controllers,
            game_controller_subsystem,
            gamepad: Gamepad::new(gamepad_cfg),
            keyboard: Keyboard::new(),
            touch: super::touch::TouchState::new(),
        })
    }

    /// Push updated gamepad tunables (dead zone, trigger/hold thresholds) into
    /// the controller state machine — used when the settings overlay changes them
    /// live (see [`crate::app::App::apply_config`]).
    pub fn set_gamepad_config(&mut self, cfg: InputConfig) {
        self.gamepad.set_config(cfg);
    }

    /// Swap in a freshly built gamepad gesture table (the settings overlay
    /// rebinding controls live; see [`crate::app::App::settings_close`]).
    pub fn set_bindings(&mut self, bindings: crate::event::bindings::Bindings) {
        self.gamepad.set_bindings(bindings);
    }

    /// Swap in a freshly built keyboard shortcut table (the keyboard half of
    /// rebinding; the gamepad half is [`Self::set_bindings`]).
    pub fn set_key_bindings(&mut self, bindings: crate::event::bindings::KeyBindings) {
        self.keyboard.set_bindings(bindings);
    }

    pub fn wait(
        &mut self,
        window: &AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // Mirror the overlay's capture state into the pad so its buttons feed the
        // gesture resolver (instead of dispatching) while a binding is captured.
        self.gamepad.set_capture(ui.settings_capturing());

        // Block for the next event only when idle. When the gamepad is active or
        // the page is animating, return promptly so the main loop keeps ticking
        // (vsync caps the rate); blocking here would stall cursor/scroll motion.
        if !browser.is_animating() && !self.gamepad.is_active() {
            match ui.take_repain_delay() {
                Some(delay) => {
                    if let Some(event) =
                        self.event_pump.wait_event_timeout(delay.as_millis() as u32)
                    {
                        self.handle_event(event, window, ui, browser, commands);
                    }
                }
                None => {
                    let event = self.event_pump.wait_event();
                    self.handle_event(event, window, ui, browser, commands);
                }
            }
        }

        // Drain everything else queued this frame (notably the flood of analog
        // stick axis events) so we always act on the latest input — no backlog lag.
        while let Some(event) = self.event_pump.poll_event() {
            self.handle_event(event, window, ui, browser, commands);
        }

        // Emit this frame's analog state as a command for the router to apply,
        // and fire any `hold:` gesture whose threshold just passed.
        self.gamepad.tick(commands);
    }

    fn handle_event(
        &mut self,
        event: Event,
        window: &AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // Settings is capturing a binding: take raw key events before egui or the
        // shortcut table sees them (egui would eat Tab/arrows/Enter/Esc, so they
        // couldn't be bound). A non-modifier KeyDown is the captured combo; Esc
        // cancels; bare modifiers and the key-up / text edge are swallowed so
        // nothing leaks to the page. (The gamepad half is captured in the pad's
        // own capture mode, synced in `wait`.)
        if ui.settings_capturing() {
            match event {
                Event::KeyDown {
                    keycode: Some(kc),
                    keymod,
                    repeat: false,
                    ..
                } => {
                    if kc == Keycode::Escape {
                        commands.push(AppCommand::Settings(SettingsAction::CaptureCancel));
                    } else if is_modifier_key(kc) {
                        return;
                    } else if let Some(gesture) = crate::event::bindings::format_key(kc, keymod) {
                        commands.push(AppCommand::Settings(SettingsAction::CaptureBinding {
                            gesture,
                            keyboard: true,
                        }));
                    }
                    return;
                }
                Event::KeyDown { .. } | Event::KeyUp { .. } | Event::TextInput { .. } => return,
                _ => {}
            }
        }

        let consumed = ui.handle_event(window, &event);

        if consumed {
            return;
        }

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                if let Ok(controller) = self.game_controller_subsystem.open(which) {
                    self.game_controllers.push(controller);
                    log::info!("Controller {which} connected");
                }
            }
            Event::ControllerDeviceRemoved { which, .. } => {
                self.game_controllers.retain(|c| c.instance_id() != which);
                log::info!("Controller {which} disconnected");
            }
            Event::MouseButtonUp {
                mouse_btn, x, y, ..
            } => {
                let (x, y) = ui.to_browser_rel_pos(x as f32, y as f32);
                let event = super::sdl2_servo::into_mouse_button_event(mouse_btn, x, y, false);
                let event = servo::InputEvent::MouseButton(event);
                browser.handle_input(event);
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                let (x, y) = ui.to_browser_rel_pos(x as f32, y as f32);
                let event = super::sdl2_servo::into_mouse_button_event(mouse_btn, x, y, true);
                let event = servo::InputEvent::MouseButton(event);

                browser.handle_input(event);
            }
            Event::MouseMotion { x, y, .. } => {
                let (x, y) = ui.to_browser_rel_pos(x as f32, y as f32);
                let event = super::sdl2_servo::into_mouse_move_event(x, y);
                let event = servo::InputEvent::MouseMove(event);
                browser.handle_input(event);
            }
            Event::MouseWheel {
                x,
                y,
                mouse_x,
                mouse_y,
                ..
            } => {
                let (mx, my) = ui.to_browser_rel_pos(mouse_x as f32, mouse_y as f32);
                // Fire the DOM `wheel` event (for pages with JS handlers)...
                let event = super::sdl2_servo::into_wheel_event(x, y, mx, my);
                browser.handle_input(servo::InputEvent::Wheel(event));
                // ...then perform the actual native scroll. SDL `y` is positive
                // when scrolling up; Servo's positive `dy` reveals lower content.
                const WHEEL_PX: f32 = 60.0;
                let dy = -y as f32 * WHEEL_PX;
                browser.scroll(-x as f32 * WHEEL_PX, dy, mx, my);
                ui.notify_page_scroll(dy);
            }
            // Touch: SDL finger coords are normalized to the window; scale to the
            // pixel space mouse events use. These only reach here for the web-view
            // area (egui consumes touch over the toolbar). A drag scrolls, a tap
            // clicks. See [`super::touch`].
            Event::FingerDown {
                finger_id, x, y, ..
            } => {
                let (w, h) = window.size();
                let (px, py) = (x * w as f32, y * h as f32);
                // Only the web view scrolls/taps from touch; toolbar touches are
                // egui's (it synthesizes pointer events from them). Starting a
                // gesture for a toolbar touch would leak (its up is consumed by
                // egui, so it never resolves) and could click the page underneath.
                if ui.point_over_webview(py) {
                    self.touch.down(finger_id, px, py);
                }
            }
            Event::FingerMotion {
                finger_id, x, y, ..
            } => {
                let (w, h) = window.size();
                let (px, py) = (x * w as f32, y * h as f32);
                if let Some((dx, dy)) = self.touch.motion(finger_id, px, py) {
                    let (bx, by) = ui.to_browser_rel_pos(px, py);
                    // Content follows the finger: dragging down reveals upper
                    // content, and Servo's positive dy reveals lower content, so
                    // negate the deltas.
                    browser.scroll(-dx, -dy, bx, by);
                    ui.notify_page_scroll(-dy);
                }
            }
            Event::FingerUp { finger_id, .. } => {
                if let super::touch::TouchEnd::Tap(px, py) = self.touch.up(finger_id) {
                    let (bx, by) = ui.to_browser_rel_pos(px, py);
                    let down = super::sdl2_servo::into_mouse_button_event(
                        sdl2::mouse::MouseButton::Left,
                        bx,
                        by,
                        true,
                    );
                    browser.handle_input(servo::InputEvent::MouseButton(down));
                    let up = super::sdl2_servo::into_mouse_button_event(
                        sdl2::mouse::MouseButton::Left,
                        bx,
                        by,
                        false,
                    );
                    browser.handle_input(servo::InputEvent::MouseButton(up));
                }
            }
            Event::KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let key = KeyEvent {
                    kc,
                    sc,
                    keymod,
                    repeat,
                    pressed: true,
                };
                self.keyboard.on_key(&key, ui, browser, commands);
            }
            Event::KeyUp {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let key = KeyEvent {
                    kc,
                    sc,
                    keymod,
                    repeat,
                    pressed: false,
                };
                self.keyboard.on_key(&key, ui, browser, commands);
            }
            Event::ControllerAxisMotion { axis, value, .. } => {
                self.gamepad.on_axis(axis, value, commands);
            }
            Event::ControllerButtonDown { button, .. } => {
                self.gamepad.on_button(button, true, commands);
            }
            Event::ControllerButtonUp { button, .. } => {
                self.gamepad.on_button(button, false, commands);
            }
            Event::Quit { .. } => commands.push(AppCommand::Shutdown),
            Event::User { code, .. } => {
                if let Some(cmd) = handle_user(code) {
                    commands.push(cmd);
                }
            }
            Event::Window { win_event, .. } => {
                if let Some(cmd) = handle_window(win_event) {
                    commands.push(cmd);
                }
            }
            _ => {}
        }
    }
}
