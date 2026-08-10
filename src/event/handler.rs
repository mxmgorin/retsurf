use super::gamepad::Gamepad;
use super::keyboard::KeyEvent;
use crate::event::bindings::{self, Action};
use crate::{
    app::{AppCommand, SettingsAction},
    browser::AppBrowser,
    config::InputConfig,
    event::{user::handle_user, window::handle_window},
    platform::window::AppWindow,
    ui::AppUi,
};
use inputbind::sdl::{is_modifier, key_name, mods_for, pad_of, KeyNames};
use inputbind::{Bindings, Capture, Captured, Store, Tick};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use std::time::{Duration, Instant};

/// Give up on an idle capture: a handheld has no Esc to cancel with.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);

pub struct AppEventHandler {
    event_pump: sdl2::EventPump,
    game_controllers: Vec<sdl2::controller::GameController>,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
    /// Gesture → action tables for both devices, from `bindings.toml`.
    bindings: Bindings<Action>,
    /// Derived once, so the `[keyboard]` table resolves its names at load.
    key_names: KeyNames,
    /// Controller state machine: sticks/triggers, tap/hold/chord gestures.
    gamepad: Gamepad,
    /// Takes input from both devices, so it lives here rather than in either.
    capture: Capture,
    /// Single-finger touch gestures (drag scrolls, tap clicks) over the web view.
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

        let key_names = KeyNames::new();
        let hold = Duration::from_millis(gamepad_cfg.hold_ms);
        Ok(Self {
            event_pump: sdl.event_pump()?,
            game_controllers,
            game_controller_subsystem,
            bindings: bindings::build(&bindings::load_store(), &key_names),
            key_names,
            gamepad: Gamepad::new(gamepad_cfg),
            capture: Capture::new(hold, CAPTURE_TIMEOUT),
            touch: super::touch::TouchState::new(),
        })
    }

    /// Push updated gamepad tunables (dead zone, trigger/hold thresholds) into
    /// the controller state machine — used when the settings overlay changes them
    /// live (see [`crate::app::App::apply_config`]).
    pub fn set_gamepad_config(&mut self, cfg: InputConfig) {
        self.capture = Capture::new(Duration::from_millis(cfg.hold_ms), CAPTURE_TIMEOUT);
        self.gamepad.set_config(cfg);
    }

    /// Rebuild both devices' tables from an edited store. The pad forgets what it
    /// holds, so a press begun under the old table cannot resolve against the new.
    pub fn set_bindings(&mut self, store: &Store, commands: &mut Vec<AppCommand>) {
        self.bindings = bindings::build(store, &self.key_names);
        self.gamepad.reset(commands);
    }

    pub fn wait(
        &mut self,
        window: &AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // The pad drops what it holds either way, so a button held across the
        // transition cannot resolve as both a gesture to bind and a bound action.
        let capturing = ui.settings.capturing();
        if capturing != self.capture.is_on() {
            self.capture
                .set(capturing, &self.gamepad.held(), Instant::now());
            self.gamepad.reset(commands);
        }

        // Block for the next event only when idle. When the gamepad is active or
        // the page is animating, return promptly so the main loop keeps ticking
        // (vsync caps the rate); blocking here would stall cursor/scroll motion.
        if !browser.is_animating() && !self.gamepad.is_active() && !self.capture.is_on() {
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

        // Capture owns the pad, so no analog state is emitted while it is open.
        if self.capture.is_on() {
            match self.capture.tick(Instant::now()) {
                Tick::Got(captured) => push_capture(commands, captured),
                Tick::GaveUp => commands.push(AppCommand::Settings(SettingsAction::CaptureCancel)),
                Tick::Waiting => {}
            }
            return;
        }
        // Emit this frame's analog state as a command for the router to apply,
        // and fire any hold or repeat whose deadline just passed.
        self.gamepad.tick(commands);
    }

    /// A raw event taken before egui sees it, which would eat Tab/arrows/Enter/Esc
    /// and leave them unbindable. Returns whether capture consumed it.
    fn on_capture_event(&mut self, event: &Event, commands: &mut Vec<AppCommand>) -> bool {
        let now = Instant::now();
        let captured = match event {
            Event::KeyDown {
                keycode: Some(kc),
                keymod,
                repeat: false,
                ..
            } => {
                // Esc cancels rather than binds: the desktop's way out.
                if *kc == Keycode::Escape {
                    commands.push(AppCommand::Settings(SettingsAction::CaptureCancel));
                    return true;
                }
                self.capture.on_key(
                    &key_name(*kc),
                    mods_for(*kc, *keymod),
                    is_modifier(*kc),
                    now,
                )
            }
            Event::KeyUp {
                keycode: Some(kc), ..
            } => self.capture.on_key_release(&key_name(*kc)),
            // Autorepeat and the text edge are swallowed, never bound.
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::TextInput { .. } => return true,
            Event::ControllerButtonDown { button, .. } => {
                pad_of(*button).and_then(|pad| self.capture.on_press(pad, now))
            }
            Event::ControllerButtonUp { button, .. } => {
                pad_of(*button).and_then(|pad| self.capture.on_release(pad, now))
            }
            // Triggers are the one bindable axis; the sticks freeze, consumed so
            // no cursor moves under the listening screen.
            Event::ControllerAxisMotion { axis, value, .. } => {
                match self.gamepad.trigger_edges(*axis, *value) {
                    (_, Some(pad)) => self.capture.on_press(pad, now),
                    (Some(pad), None) => self.capture.on_release(pad, now),
                    (None, None) => None,
                }
            }
            _ => return false,
        };
        if let Some(captured) = captured {
            push_capture(commands, captured);
        }
        true
    }

    fn handle_event(
        &mut self,
        event: Event,
        window: &AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if self.capture.is_on() && self.on_capture_event(&event, commands) {
            return;
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
                // Remember the input came from the keyboard so hint mode picks
                // typed-letter badges when it opens (see `AppUi::note_input_keyboard`).
                ui.note_input_keyboard(true);
                super::keyboard::on_key(&key, &self.bindings, ui, browser, commands);
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
                super::keyboard::on_key(&key, &self.bindings, ui, browser, commands);
            }
            Event::ControllerAxisMotion { axis, value, .. } => {
                self.gamepad.on_axis(axis, value, &self.bindings, commands);
            }
            Event::ControllerButtonDown { button, .. } => {
                // A pad press reclaims hint badges as button combos (see KeyDown).
                ui.note_input_keyboard(false);
                self.gamepad
                    .on_button(button, true, &self.bindings, commands);
            }
            Event::ControllerButtonUp { button, .. } => {
                self.gamepad
                    .on_button(button, false, &self.bindings, commands);
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

/// `keyboard` tells the tables apart: their gesture text collides (`"a"` is both).
fn push_capture(commands: &mut Vec<AppCommand>, captured: Captured) {
    let (gesture, keyboard) = match captured {
        Captured::Pad(gesture) => (gesture.to_text(), false),
        Captured::Key(gesture) => (gesture.to_text(), true),
    };
    commands.push(AppCommand::Settings(SettingsAction::CaptureBinding {
        gesture,
        keyboard,
    }));
}
