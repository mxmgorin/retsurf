use super::game::input_map::{Side, STICK_PREFIX};
use super::game::mode::GameInput;
use super::game::{self, GameMode, DEFAULT_EXIT};
use super::gamepad::Gamepad;
use super::gamepad_api;
use super::key_names;
use super::keyboard::KeyEvent;
use crate::event::bindings::{self, Action};
use crate::{
    browser::AppBrowser,
    command::{AppCommand, GameMapEditAction, InputCommand, SettingsAction},
    config::InputConfig,
    event::gamepad::labelled_pad,
    event::window::handle_window,
    overlay::osk::PadInput,
    platform::window::AppWindow,
    ui::{AppUi, Focus},
};
use inputbind::sdl::{axis_value, is_modifier, key_code, key_name, mods_for, trigger_of, Keymap};
use inputbind::{Bindings, Capture, Captured, Pad, PadGesture, Store, Tick};
use sdl2::controller::Axis;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use std::time::{Duration, Instant};

/// The gesture Game Mode reserves against every map. Falls back where the file
/// binds `game_mode` to nothing on the pad, so a session always has a way out.
fn game_exit_gesture(store: &Store) -> PadGesture {
    bindings::pad_gesture(store, Action::GameMode).unwrap_or(DEFAULT_EXIT)
}

/// Give up on an idle capture: a handheld has no Esc to cancel with.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);

/// How far a stick must be pushed to be captured: past any dead zone, so one
/// resting off-centre cannot bind itself.
const CAPTURE_DEFLECTION: f32 = 0.7;

/// Longest an animating page's pass waits on the queue. Only a wake Servo failed
/// to send is paid at this rate; a delivered frame returns the wait at once.
const ANIMATION_WAIT: Duration = Duration::from_millis(16);

/// Longest an idle pass waits. Bounded because sdl2's unbounded `wait_event`
/// panics when SDL reports a failed wait, which an interrupted poll is enough for.
const IDLE_WAIT: Duration = Duration::from_millis(500);

/// Cap on one rumble effect, matching Chrome; SDL wants milliseconds.
const MAX_RUMBLE_MS: f64 = 5000.0;

/// A spec magnitude (0..1) as SDL's u16 motor intensity.
fn rumble_magnitude(magnitude: f64) -> u16 {
    (magnitude.clamp(0.0, 1.0) * f64::from(u16::MAX)).round() as u16
}

/// A screen listening for a gesture to bind: the machine both devices feed, and
/// the latch for the one source it does not speak for.
struct Listening {
    capture: Capture,
    /// Whether a stick was already reported this round. [`Capture`] speaks for
    /// pads and keys, so the source it does not know needs its own latch.
    stick: bool,
}

impl Listening {
    /// Start listening. `held` is the pads down right now — the press that
    /// activated the row, which must not be taken as the binding.
    fn new(cfg: &InputConfig, held: &[Pad], now: Instant) -> Self {
        let mut capture = Capture::new(Duration::from_millis(cfg.hold_ms), CAPTURE_TIMEOUT);
        capture.set(true, held, now);
        Self {
            capture,
            stick: false,
        }
    }
}

pub struct AppEventHandler {
    event_pump: sdl2::EventPump,
    game_controllers: Vec<sdl2::controller::GameController>,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
    /// Gesture-to-action tables for both devices, from `bindings.toml`.
    bindings: Bindings<Action>,
    /// The pad gesture `game_mode` answers to. Held apart from the tables because
    /// Game Mode bypasses them and matches this one gesture on its own.
    game_exit: PadGesture,
    /// Controller state machine: sticks/triggers, tap/hold/chord gestures.
    gamepad: Gamepad,
    /// Whether the keys arriving from this device *are* the pad. The Miyoo's
    /// SDL2 offers no controller mapping and sends keys instead, so there they
    /// feed the pad machine and answer to the `[gamepad]` table like any button.
    keymap: Keymap,
    /// MENU is the launcher's key nearly everywhere, so the app answers it only
    /// where the launcher hands it over (`RETSURF_MENU_QUIT`, set by both Miyoo
    /// packages: a firmware kill helper writes no session on the way out).
    menu_quits: bool,
    /// The binding screen listening for a gesture, if one is. Takes input from
    /// both devices, so it lives here rather than in either.
    listening: Option<Listening>,
    /// Game Mode, for as long as the mode or one of its screens is up (see
    /// [`Self::wait`]).
    game: Option<GameMode>,
    /// Single-finger touch gestures (drag scrolls, tap clicks) over the web view.
    touch: super::touch::TouchState,
}

impl AppEventHandler {
    pub fn new(sdl: &sdl2::Sdl, gamepad_cfg: InputConfig) -> Result<Self, String> {
        let mut game_controllers = vec![];
        let game_controller_subsystem = sdl.game_controller()?;
        // `RETSURF_KEYMAP` wins over the driver name, and has to: the bundled
        // Miyoo SDL2 calls its driver `Mini`, not the `mmiyoo` detection knows.
        let keymap = Keymap::detect(
            sdl.video()?.current_video_driver(),
            std::env::var("RETSURF_KEYMAP").ok().as_deref(),
        );
        log::info!("keyboard layout: {keymap:?}");
        let menu_quits = crate::config::env_flag("RETSURF_MENU_QUIT").unwrap_or(false);

        for id in 0..game_controller_subsystem.num_joysticks()? {
            if game_controller_subsystem.is_game_controller(id) {
                let controller = game_controller_subsystem.open(id).unwrap();
                game_controllers.push(controller);
            }
        }

        let store = bindings::load_store();
        Ok(Self {
            event_pump: sdl.event_pump()?,
            game_controllers,
            game_controller_subsystem,
            game_exit: game_exit_gesture(&store),
            bindings: bindings::build(&store, &key_names()),
            gamepad: Gamepad::new(gamepad_cfg),
            keymap,
            menu_quits,
            listening: None,
            game: None,
            touch: super::touch::TouchState::new(),
        })
    }

    /// Push updated gamepad tunables (dead zone, trigger/hold thresholds) into
    /// the controller state machine — used when the settings overlay changes them
    /// live (see [`crate::app::App::apply_config`]).
    pub fn set_gamepad_config(&mut self, cfg: InputConfig) {
        if let Some(game) = &mut self.game {
            game.set_config(&cfg);
        }
        self.gamepad.set_config(cfg);
    }

    /// Game Mode, loaded on demand. `start` is the map to run then, which only
    /// the caller has: the config is not the handler's.
    pub fn game_mut(&mut self, start: &str) -> &mut GameMode {
        let exit = self.game_exit;
        let game = self.game.get_or_insert_with(|| GameMode::load(start));
        game.set_exit(exit);
        game
    }

    /// Game Mode where it has already loaded, for the paths that must not be
    /// what loads it.
    pub fn game(&mut self) -> Option<&mut GameMode> {
        self.game.as_mut()
    }

    /// The translator while Game Mode holds the pad, and `None` while the
    /// chrome does.
    fn routing_input(&mut self) -> Option<&mut GameInput> {
        self.game.as_mut().and_then(GameMode::routing)
    }

    /// Whether the pad routed to the game last pass.
    fn routing(&self) -> bool {
        self.game.as_ref().is_some_and(GameMode::is_routing)
    }

    /// Rebuild both devices' tables from an edited store. The pad forgets what it
    /// holds, so a press begun under the old table cannot resolve against the new.
    pub fn set_bindings(&mut self, store: &Store, commands: &mut Vec<AppCommand>) {
        self.bindings = bindings::build(store, &key_names());
        self.game_exit = game_exit_gesture(store);
        if let Some(game) = &mut self.game {
            game.set_exit(self.game_exit);
        }
        self.gamepad.reset(commands);
    }

    /// The button the Game Mode gesture takes outright, which no map may bind.
    pub fn game_spent_pad(&self) -> Option<Pad> {
        game::spent_pad(self.game_exit)
    }

    /// How the pad reaches the Game Mode menu, or `None` where this device has no
    /// pad to name.
    pub fn game_exit_text(&self) -> Option<String> {
        self.has_pad().then(|| self.game_exit.to_text())
    }

    /// Whether this device has a pad at all — a controller, or a panel that
    /// wires its buttons to keys (the Miyoos).
    pub fn has_pad(&self) -> bool {
        !self.game_controllers.is_empty() || self.keymap != Keymap::Desktop
    }

    /// Reports whether this pass waited on the event queue. The loop's other
    /// pacing ([`crate::app::App::pace_frame`]) is for the passes that did not.
    pub fn wait(
        &mut self,
        window: &mut AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        // The pad drops what it holds either way, so a button held across the
        // transition cannot resolve as both a gesture to bind and a bound action.
        // Two screens bind by listening — the settings' Controls rows and the
        // Game Mode map editor — and one machine serves both.
        let capturing = ui.settings.capturing() || ui.map_edit.capturing();
        if capturing != self.listening.is_some() {
            let held = self.gamepad.held();
            self.listening =
                capturing.then(|| Listening::new(&self.gamepad.cfg, &held, Instant::now()));
            self.gamepad.reset(commands);
        }

        // Game Mode routes the pad to the game while the page owns the focus; on
        // the way out everything the page holds is released, so no key sticks.
        let game_on = ui.game_mode() && ui.focus() == Focus::Page && self.listening.is_none();
        if game_on != self.routing() {
            // Nothing to route through until a map screen has loaded one, and
            // entering the mode goes through its menu.
            if let Some(game) = &mut self.game {
                match game_on {
                    true => {
                        game.start_routing(&self.gamepad.cfg);
                        self.gamepad.reset(commands);
                    }
                    false => game.stop_routing(browser, commands),
                }
            }
        }

        // Nothing is holding the maps: the mode is off and none of its screens is
        // up. Released on the way out, so dropping them can strand no key.
        if !ui.game_mode() && !ui.game_screen() {
            if let Some(mut game) = self.game.take() {
                game.stop_routing(browser, commands);
            }
        }

        // An active pad returns promptly: it drives the cursor from a held stick,
        // which produces no event to wake on, so blocking would stall the motion.
        let device_active = match self.routing_input() {
            Some(input) => input.is_active(),
            None => self.gamepad.is_active(),
        };
        let waited = !device_active && self.listening.is_none();
        if waited {
            // An animating page waits too: Servo rings the queue through its
            // event-loop waker on every paint message, so this wakes on the frame.
            let delay = ui.take_repaint_delay();
            let delay = if browser.is_animating() {
                Some(delay.map_or(ANIMATION_WAIT, |delay| delay.min(ANIMATION_WAIT)))
            } else {
                delay
            };
            let delay = delay.unwrap_or(IDLE_WAIT);
            if let Some(event) = self.event_pump.wait_event_timeout(delay.as_millis() as u32) {
                self.handle_event(event, window, ui, browser, commands);
            }
        }

        // Drain everything else queued this frame (notably the flood of analog
        // stick axis events) so we always act on the latest input — no backlog lag.
        while let Some(event) = self.event_pump.poll_event() {
            self.handle_event(event, window, ui, browser, commands);
        }

        // Capture owns the pad, so no analog state is emitted while it is open.
        if let Some(listening) = &mut self.listening {
            let for_map = ui.map_edit.capturing();
            match listening.capture.tick(Instant::now()) {
                Tick::Got(captured) => push_capture(commands, captured, for_map),
                Tick::GaveUp => push_capture_cancel(commands, for_map),
                Tick::Waiting => {}
            }
            return waited;
        }
        // Emit this frame's analog state as a command for the router to apply,
        // and fire any hold or repeat whose deadline just passed.
        match self.routing_input() {
            Some(input) => input.tick(browser, commands),
            None => self.gamepad.tick(commands),
        }
        waited
    }

    /// Announce the pads that were already plugged in at startup. They arrive
    /// through no `ControllerDeviceAdded`, and without a `Connected` the engine
    /// has no `Gamepad` object to route their input to.
    pub fn announce_pads(&mut self, browser: &AppBrowser) {
        for (id, name) in self
            .game_controllers
            .iter()
            .map(|controller| (controller.instance_id(), controller.name()))
            .collect::<Vec<_>>()
        {
            browser.pad_connected(id, name);
        }
    }

    /// Play or stop a page's rumble on the pad it named. Every request reports
    /// success: the engine reads `false` as "superseded, settled elsewhere", so
    /// a bare failure strands the page's promise; a motorless pad completes.
    pub fn haptic(&mut self, browser: &AppBrowser, request: servo::GamepadHapticEffectRequest) {
        use servo::{GamepadHapticEffectRequestType, GamepadHapticEffectType};
        let controller = browser
            .pad_instance(request.gamepad_index())
            .and_then(|id| {
                self.game_controllers
                    .iter_mut()
                    .find(|c| c.instance_id() == id)
            });
        let Some(controller) = controller else {
            log::debug!("rumble: pad slot {} is gone", request.gamepad_index());
            return request.succeeded();
        };
        let (low, high, ms) = match request.request_type() {
            GamepadHapticEffectRequestType::Play(GamepadHapticEffectType::DualRumble(params)) => {
                // The spec's strong magnitude is the low-frequency motor. SDL has
                // no start delay; the effect simply plays now.
                (
                    rumble_magnitude(params.strong_magnitude),
                    rumble_magnitude(params.weak_magnitude),
                    params.duration.clamp(0.0, MAX_RUMBLE_MS) as u32,
                )
            }
            GamepadHapticEffectRequestType::Stop => (0, 0, 0),
        };
        let instance_id = controller.instance_id();
        match controller.set_rumble(low, high, ms) {
            Ok(()) => log::debug!("rumble: pad {instance_id} low {low} high {high} for {ms} ms"),
            // A pad without motors: the effect "completes" silently.
            Err(err) => log::debug!("rumble unavailable on pad {instance_id}: {err}"),
        }
        request.succeeded();
    }

    /// A button in Game Mode: through the translator, and to the Gamepad API
    /// only when the translator leaves the source unbound.
    fn game_button(
        &mut self,
        instance_id: u32,
        button: sdl2::controller::Button,
        pressed: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        // The buttons a map sends ride the pad that drove them, so the page
        // reads a remap as the same device rather than a second one.
        let swap = self.gamepad.cfg.swap_face_buttons;
        let withheld = self.routing_input().is_some_and(|input| {
            input.note_pad_slot(browser.pad_slot(instance_id));
            labelled_pad(button, swap)
                .is_some_and(|pad| input.on_pad(pad, pressed, browser, commands))
        });
        if !withheld {
            self.to_page(browser, instance_id, |slot| {
                gamepad_api::button(slot, button, pressed)
            });
        }
    }

    /// Hand the page a pad event alongside the chrome's own reading of it. The
    /// Gamepad API is polled, so nothing is taken from the chrome by doing both.
    fn to_page(
        &self,
        browser: &AppBrowser,
        instance_id: u32,
        event: impl FnOnce(usize) -> Option<servo::GamepadEvent>,
    ) {
        let Some(slot) = browser.pad_slot(instance_id) else {
            return;
        };
        if let Some(event) = event(slot) {
            browser.handle_input(servo::InputEvent::Gamepad(event));
        }
    }

    /// A raw event taken before egui sees it, which would eat Tab/arrows/Enter/Esc
    /// and leave them unbindable. Returns whether capture consumed it. `for_map`
    /// is which screen asked to listen, since the answer goes back to it.
    fn on_capture_event(
        &mut self,
        event: &Event,
        for_map: bool,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        let now = Instant::now();
        let Some(listening) = self.listening.as_mut() else {
            return false;
        };
        let captured = match event {
            Event::KeyDown {
                keycode: Some(kc),
                keymod,
                repeat: false,
                ..
            } => {
                // Esc cancels rather than binds: the desktop's way out.
                if *kc == Keycode::Escape {
                    push_capture_cancel(commands, for_map);
                    return true;
                }
                // Where the pad arrives as keys, it binds as the pad it is.
                if let Some(pad) = self.keymap.pad(*kc) {
                    listening.capture.on_press(pad, now)
                } else {
                    listening.capture.on_key(
                        &key_name(*kc),
                        mods_for(*kc, *keymod),
                        is_modifier(*kc),
                        now,
                    )
                }
            }
            Event::KeyUp {
                keycode: Some(kc), ..
            } => match self.keymap.pad(*kc) {
                Some(pad) => listening.capture.on_release(pad, now),
                None => listening.capture.on_key_release(&key_name(*kc)),
            },
            // Autorepeat and the text edge are swallowed, never bound.
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::TextInput { .. } => return true,
            Event::ControllerButtonDown { button, .. } => {
                labelled_pad(*button, self.gamepad.cfg.swap_face_buttons)
                    .and_then(|pad| listening.capture.on_press(pad, now))
            }
            Event::ControllerButtonUp { button, .. } => {
                labelled_pad(*button, self.gamepad.cfg.swap_face_buttons)
                    .and_then(|pad| listening.capture.on_release(pad, now))
            }
            // A trigger binds as the button it is, a stick only where a map is
            // listening; both are consumed, so nothing moves underneath.
            Event::ControllerAxisMotion { axis, value, .. } => {
                match self.gamepad.trigger_edges(*axis, *value) {
                    (_, Some(pad)) => listening.capture.on_press(pad, now),
                    (Some(pad), None) => listening.capture.on_release(pad, now),
                    (None, None) => {
                        let armed = for_map && listening.capture.is_armed();
                        if armed && !listening.stick {
                            if let Some(side) = pushed_stick(*axis, *value) {
                                listening.stick = true;
                                push_capture_stick(commands, side);
                            }
                        }
                        None
                    }
                }
            }
            _ => return false,
        };
        if let Some(captured) = captured {
            push_capture(commands, captured, for_map);
        }
        true
    }

    /// Whether the event is the pad arriving as keys ([`Keymap`]), which egui must
    /// not see: it typed a space into the focused field on every A press, and ate
    /// its last letter on R2 (the Miyoo sends R2 as Backspace).
    fn is_pad_as_keys(&self, event: &Event) -> bool {
        if self.keymap == Keymap::Desktop {
            return false;
        }
        match event {
            Event::TextInput { .. } | Event::TextEditing { .. } => true,
            Event::KeyDown {
                keycode: Some(kc), ..
            }
            | Event::KeyUp {
                keycode: Some(kc), ..
            } => self.keymap.pad(*kc).is_some(),
            _ => false,
        }
    }

    /// A key while Game Mode has the page: the mode's own gesture first, then
    /// the map's own table, then the game — which is where the rest go.
    fn game_key(&mut self, key: &KeyEvent, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        let code = key_code(key.kc);
        if self.bindings.key(code, mods_for(key.kc, key.keymod)) == Some(Action::GameMode) {
            // Both edges while the chord holds. An up after the modifiers drop
            // leaks, as every consumed binding's up already does (measured: no-op).
            if key.pressed && !key.repeat {
                commands.push(AppCommand::GameMode);
            }
            return;
        }
        let taken = self
            .routing_input()
            .is_some_and(|input| input.on_key(code, key.pressed, key.repeat, browser, commands));
        if taken {
            return;
        }
        let event = super::keyboard::into_servo(key);
        browser.handle_input(servo::InputEvent::Keyboard(event));
    }

    /// One key edge, either direction: a key-wired pad feeds the pad machine,
    /// anything else the keyboard path — Game Mode first when it holds the device.
    fn on_key_event(
        &mut self,
        key: KeyEvent,
        ui: &mut AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if let Some(pad) = self.keymap.pad(key.kc) {
            if let Some(input) = self.routing_input() {
                input.on_pad(pad, key.pressed, browser, commands);
                return;
            }
            // A pad press reclaims hint badges as button combos.
            if key.pressed {
                ui.note_input_keyboard(false);
            }
            self.pad_edge(pad, key.pressed, ui, commands);
            return;
        }
        // Remember the input came from the keyboard so hint mode picks
        // typed-letter badges when it opens (see `AppUi::note_input_keyboard`).
        if key.pressed {
            ui.note_input_keyboard(true);
        }
        if self.routing() {
            self.game_key(&key, browser, commands);
            return;
        }
        super::keyboard::on_key(&key, &self.bindings, ui, browser, commands);
    }

    /// One pad-button edge, either direction — the controller twin of
    /// [`Self::on_key_event`].
    /// One pad edge: the keyboard's while it has focus and wants it, the
    /// bindings' otherwise. A press the pad machine saw is released there too.
    fn pad_edge(
        &mut self,
        pad: Pad,
        pressed: bool,
        ui: &mut AppUi,
        commands: &mut Vec<AppCommand>,
    ) {
        let owed = !pressed && self.gamepad.held().contains(&pad);
        if !owed && osk_claims(pad, pressed, ui, commands) {
            return;
        }
        self.gamepad.on_pad(pad, pressed, &self.bindings, commands);
    }

    fn on_pad_button(
        &mut self,
        which: u32,
        button: sdl2::controller::Button,
        pressed: bool,
        ui: &mut AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if self.routing() {
            self.game_button(which, button, pressed, browser, commands);
            return;
        }
        // A pad press reclaims hint badges as button combos (see the key path).
        if pressed {
            ui.note_input_keyboard(false);
        }
        self.to_page(browser, which, |slot| {
            gamepad_api::button(slot, button, pressed)
        });
        if let Some(pad) = labelled_pad(button, self.gamepad.cfg.swap_face_buttons) {
            self.pad_edge(pad, pressed, ui, commands);
        }
    }

    fn handle_event(
        &mut self,
        event: Event,
        window: &mut AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if self.listening.is_some()
            && self.on_capture_event(&event, ui.map_edit.capturing(), commands)
        {
            return;
        }

        // A modified key stays ours: egui reports *every* key consumed while a
        // text field has focus. Game Mode's keys belong to the page, not egui.
        let egui_first = !self.is_pad_as_keys(&event) && !(self.routing() && is_key(&event));
        if egui_first && ui.handle_event(window, &event) && !is_shortcut_key(&event) {
            return;
        }

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                if let Ok(controller) = self.game_controller_subsystem.open(which) {
                    let (id, name) = (controller.instance_id(), controller.name());
                    self.game_controllers.push(controller);
                    log::info!("Controller {which} connected");
                    browser.pad_connected(id, name);
                }
            }
            Event::ControllerDeviceRemoved { which, .. } => {
                self.game_controllers.retain(|c| c.instance_id() != which);
                log::info!("Controller {which} disconnected");
                browser.pad_disconnected(which);
            }
            Event::MouseButtonUp {
                mouse_btn, x, y, ..
            } => {
                let (x, y) = ui.to_browser_rel_pos(x as f32, y as f32);
                browser.mouse_button(mouse_btn, x, y, false);
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                let (x, y) = ui.to_browser_rel_pos(x as f32, y as f32);
                browser.mouse_button(mouse_btn, x, y, true);
            }
            Event::MouseMotion { x, y, .. } => {
                let (x, y) = ui.to_browser_rel_pos(x as f32, y as f32);
                browser.mouse_move(x, y);
            }
            Event::MouseWheel { x, y, .. } => {
                // The event carries the pointer only from SDL 2.26, and the Linux
                // builds link whatever SDL2 the system has; ask for it instead.
                let pointer = self.event_pump.mouse_state();
                let (mx, my) = ui.to_browser_rel_pos(pointer.x() as f32, pointer.y() as f32);
                // Fire the DOM `wheel` event (for pages with JS handlers)...
                browser.wheel(x, y, mx, my);
                // ...then perform the actual native scroll. SDL `y` is positive
                // when scrolling up; Servo's positive `dy` reveals lower content.
                const WHEEL_STEP: f32 = 60.0;
                let dy = -y as f32 * WHEEL_STEP;
                ui.scroll_page(browser, -x as f32 * WHEEL_STEP, dy, mx, my);
            }
            // SDL finger coords are normalized to the window; scale them to the
            // pixel space mouse events use. See [`super::touch`].
            Event::FingerDown {
                finger_id, x, y, ..
            } => {
                let (w, h) = window.size();
                let (px, py) = (x * w as f32, y * h as f32);
                // Toolbar touches are egui's, and a gesture started for one would
                // leak: egui consumes its up, so the gesture never resolves.
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
                    let (dx, dy) = ui.to_points(dx, dy);
                    // Content follows the finger, and Servo's positive dy reveals
                    // lower content, so the deltas are negated.
                    ui.scroll_page(browser, -dx, -dy, bx, by);
                }
            }
            Event::FingerUp { finger_id, .. } => {
                if let super::touch::TouchEnd::Tap(px, py) = self.touch.up(finger_id) {
                    let (bx, by) = ui.to_browser_rel_pos(px, py);
                    for down in [true, false] {
                        browser.mouse_button(sdl2::mouse::MouseButton::Left, bx, by, down);
                    }
                }
            }
            Event::KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                // MENU, where the device sends it as Esc. Not a binding: it is
                // the only way out there, so no edit may take it away.
                if self.menu_quits && kc == Keycode::Escape && self.keymap != Keymap::Desktop {
                    commands.push(AppCommand::Shutdown);
                    return;
                }
                let key = KeyEvent {
                    kc,
                    sc,
                    keymod,
                    repeat,
                    pressed: true,
                };
                self.on_key_event(key, ui, browser, commands);
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
                self.on_key_event(key, ui, browser, commands);
            }
            Event::ControllerAxisMotion {
                which, axis, value, ..
            } => {
                // In Game Mode a bound source is withheld from the Gamepad API,
                // so a press the translator turned into a key is never seen twice.
                if self.routing() {
                    let taken = self
                        .routing_input()
                        .is_some_and(|input| input.on_axis(axis, value, browser, commands));
                    if !taken {
                        self.to_page(browser, which, |slot| gamepad_api::axis(slot, axis, value));
                    }
                    return;
                }
                self.to_page(browser, which, |slot| gamepad_api::axis(slot, axis, value));
                if trigger_of(axis).is_some() {
                    let (released, pressed) = self.gamepad.trigger_edges(axis, value);
                    for (pad, down) in [(released, false), (pressed, true)] {
                        if let Some(pad) = pad {
                            self.pad_edge(pad, down, ui, commands);
                        }
                    }
                    return;
                }
                self.gamepad.on_axis(axis, value);
            }
            Event::ControllerButtonDown { which, button, .. } => {
                self.on_pad_button(which, button, true, ui, browser, commands);
            }
            Event::ControllerButtonUp { which, button, .. } => {
                self.on_pad_button(which, button, false, ui, browser, commands);
            }
            Event::Quit { .. } => commands.push(AppCommand::Shutdown),
            // Every user event is a pure wake ([`UserEvent`]); the pass this
            // event started runs the per-frame drains, which is the delivery.
            Event::User { .. } => {}
            Event::Window { win_event, .. } => {
                if let Some(cmd) = handle_window(win_event) {
                    commands.push(cmd);
                }
            }
            _ => {}
        }
    }
}

/// Offer a pad edge to the keyboard ahead of the bindings; whether it took it.
fn osk_claims(pad: Pad, pressed: bool, ui: &mut AppUi, commands: &mut Vec<AppCommand>) -> bool {
    let Some(input) = pad_input(pad, pressed) else {
        return false;
    };
    // Decided on the press's meaning, so a release follows its press.
    let press = pad_input(pad, true).expect("a pad with a release edge has a press edge");
    if !ui.osk_takes(press) {
        return false;
    }
    let edge = match input {
        PadInput::LeftTrigger(_) => true,
        _ => pressed,
    };
    if edge {
        commands.push(AppCommand::Input(InputCommand::OskButton(input)));
    }
    true
}

/// The keyboard's reading of a pad edge; `None` for pads it has no use for.
fn pad_input(pad: Pad, pressed: bool) -> Option<PadInput> {
    Some(match pad {
        Pad::A => PadInput::A,
        Pad::B => PadInput::B,
        Pad::X => PadInput::X,
        Pad::Y => PadInput::Y,
        Pad::L1 => PadInput::Shoulder(-1),
        Pad::R1 => PadInput::Shoulder(1),
        Pad::L2 => PadInput::LeftTrigger(pressed),
        Pad::R2 => PadInput::RightTrigger,
        Pad::Start => PadInput::Start,
        Pad::Select => PadInput::Select,
        Pad::R3 => PadInput::R3,
        _ => {
            let (dx, dy) = pad.vector()?;
            PadInput::Dpad(dx, dy)
        }
    })
}

/// Anything the keyboard produces, including the text edge SDL derives from it.
fn is_key(event: &Event) -> bool {
    matches!(
        event,
        Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::TextInput { .. }
            | Event::TextEditing { .. }
    )
}

/// `keyboard` tells the tables apart: their gesture text collides (`"a"` is both).
/// A key event carrying Ctrl or Alt — a shortcut, not typing.
fn is_shortcut_key(event: &Event) -> bool {
    let keymod = match event {
        Event::KeyDown { keymod, .. } | Event::KeyUp { keymod, .. } => keymod,
        _ => return false,
    };
    use sdl2::keyboard::Mod;
    keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LALTMOD | Mod::RALTMOD)
}

/// Hand a captured gesture to whichever screen is listening.
fn push_capture(commands: &mut Vec<AppCommand>, captured: Captured, for_map: bool) {
    let (gesture, keyboard) = match captured {
        Captured::Pad(gesture) => (gesture.to_text(), false),
        Captured::Key(gesture) => (gesture.to_text(), true),
    };
    commands.push(match for_map {
        true => AppCommand::GameMapEdit(GameMapEditAction::Capture { gesture, keyboard }),
        false => AppCommand::Settings(SettingsAction::CaptureBinding { gesture, keyboard }),
    });
}

/// Which stick an axis belongs to, once pushed past [`CAPTURE_DEFLECTION`].
fn pushed_stick(axis: Axis, value: i16) -> Option<Side> {
    let side = match axis {
        Axis::LeftX | Axis::LeftY => Side::Left,
        Axis::RightX | Axis::RightY => Side::Right,
        _ => return None,
    };
    (axis_value(value).abs() >= CAPTURE_DEFLECTION).then_some(side)
}

/// Hand a pushed stick to the map editor, the one screen that binds one.
fn push_capture_stick(commands: &mut Vec<AppCommand>, side: Side) {
    commands.push(AppCommand::GameMapEdit(GameMapEditAction::Capture {
        gesture: format!("{STICK_PREFIX}{}", side.name()),
        keyboard: false,
    }));
}

/// Tell it nothing was captured, so it stops listening (Esc / the give-up).
fn push_capture_cancel(commands: &mut Vec<AppCommand>, for_map: bool) {
    commands.push(match for_map {
        true => AppCommand::GameMapEdit(GameMapEditAction::CaptureCancel),
        false => AppCommand::Settings(SettingsAction::CaptureCancel),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stick is captured only once pushed, and either axis names the whole
    /// stick.
    #[test]
    fn a_stick_is_captured_only_once_it_is_pushed() {
        let drift = (f32::from(i16::MAX) * CAPTURE_DEFLECTION / 2.0) as i16;
        assert_eq!(pushed_stick(Axis::LeftY, -i16::MAX), Some(Side::Left));
        assert_eq!(pushed_stick(Axis::RightX, i16::MAX), Some(Side::Right));
        assert_eq!(pushed_stick(Axis::LeftX, drift), None);
        // A trigger is a button, and capture takes it as one.
        assert_eq!(pushed_stick(Axis::TriggerLeft, i16::MAX), None);
    }
}
