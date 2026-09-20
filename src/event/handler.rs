use super::game::map_library::MapLibrary;
use super::game::mode::GameInput;
use super::gamepad::Gamepad;
use super::gamepad_api;
use super::keyboard::KeyEvent;
use crate::event::bindings::{self, Action};
use crate::{
    browser::AppBrowser,
    command::{AppCommand, SettingsAction},
    config::{GameModeConfig, InputConfig},
    event::window::handle_window,
    platform::window::AppWindow,
    ui::{AppUi, Focus},
};
use inputbind::sdl::{is_modifier, key_code, key_name, mods_for, pad_of, KeyNames, Keymap};
use inputbind::{Action as _, Bindings, Capture, Captured, Store, Tick};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use std::time::{Duration, Instant};

/// Give up on an idle capture: a handheld has no Esc to cancel with.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);

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

pub struct AppEventHandler {
    event_pump: sdl2::EventPump,
    game_controllers: Vec<sdl2::controller::GameController>,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
    /// Gesture-to-action tables for both devices, from `bindings.toml`.
    bindings: Bindings<Action>,
    /// The text the tables were built from, so the chrome can name a gesture
    /// the way the file spells it (see [`Self::key_gestures`]).
    store: Store,
    /// Derived once, so the `[keyboard]` table resolves its names at load.
    key_names: KeyNames,
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
    /// Takes input from both devices, so it lives here rather than in either.
    capture: Capture,
    /// Game Mode's translator: the pad and the keyboard drive the game.
    game_input: GameInput,
    /// The maps this run offers; which one runs live is [`Self::game_input`]'s.
    pub maps: MapLibrary,
    /// Whether the pad routed to the game last pass, to release on a transition.
    game_active: bool,
    /// Single-finger touch gestures (drag scrolls, tap clicks) over the web view.
    touch: super::touch::TouchState,
}

impl AppEventHandler {
    pub fn new(
        sdl: &sdl2::Sdl,
        gamepad_cfg: InputConfig,
        game_mode: &GameModeConfig,
    ) -> Result<Self, String> {
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

        let key_names = KeyNames::new();
        let hold = Duration::from_millis(gamepad_cfg.hold_ms);
        let maps = MapLibrary::load(&key_names);
        let map = maps.pick(&game_mode.input_map);
        if map.id != game_mode.input_map {
            log::warn!(
                "input map: no `{}`; using `{}`",
                game_mode.input_map,
                map.id
            );
        }
        let game_input = GameInput::new(map.clone(), &gamepad_cfg);
        let store = bindings::load_store();
        Ok(Self {
            event_pump: sdl.event_pump()?,
            game_controllers,
            game_controller_subsystem,
            bindings: bindings::build(&store, &key_names),
            store,
            key_names,
            gamepad: Gamepad::new(gamepad_cfg),
            keymap,
            menu_quits,
            capture: Capture::new(hold, CAPTURE_TIMEOUT),
            game_input,
            maps,
            game_active: false,
            touch: super::touch::TouchState::new(),
        })
    }

    /// Push updated gamepad tunables (dead zone, trigger/hold thresholds) into
    /// the controller state machine — used when the settings overlay changes them
    /// live (see [`crate::app::App::apply_config`]).
    pub fn set_gamepad_config(&mut self, cfg: InputConfig) {
        self.capture = Capture::new(Duration::from_millis(cfg.hold_ms), CAPTURE_TIMEOUT);
        self.game_input.set_config(&cfg);
        self.gamepad.set_config(cfg);
    }

    /// Write an edited map to its file (see [`MapLibrary::save`]) and re-adopt
    /// it if it is the one running. Returns its name.
    pub fn save_input_map(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> String {
        let Some(name) = self.maps.save(id, &self.key_names) else {
            return self.input_map_name().to_string();
        };
        self.readopt_input_map(id, browser, commands);
        name
    }

    /// Rename what the menu shows and write it.
    pub fn rename_input_map(
        &mut self,
        id: &str,
        name: String,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        self.maps.set_name(id, name);
        self.save_input_map(id, browser, commands);
    }

    pub fn duplicate_input_map(&mut self, id: &str, name: String) -> String {
        self.maps.duplicate(id, name, &self.key_names)
    }

    pub fn new_input_map(&mut self, name: String) -> String {
        self.maps.add_passthrough(name, &self.key_names)
    }

    /// Delete a map (see [`MapLibrary::delete`]). The mode cannot run what is
    /// no longer there, so it takes the first map instead; returns what it
    /// runs now.
    pub fn delete_input_map(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> (String, String) {
        if self.maps.delete(id, &self.key_names) && self.game_input.map_id() == id {
            let map = self.maps.pick(id).clone();
            self.game_input.set_map(map, browser, commands);
        }
        self.live_input_map()
    }

    /// The map driving the mode right now, as the menu shows it.
    pub fn input_map_name(&self) -> &str {
        &self.maps.pick(self.game_input.map_id()).name
    }

    pub fn input_map_id(&self) -> &str {
        self.game_input.map_id()
    }

    /// Hand Game Mode the map `id` names, live: what the page holds under
    /// the old one is released first. An id nothing answers to falls back to
    /// the first, so an edited config is never a dead mode.
    pub fn use_input_map(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> (String, String) {
        let map = self.maps.pick(id).clone();
        let named = (map.id.clone(), map.name.clone());
        self.game_input.set_map(map, browser, commands);
        named
    }

    fn live_input_map(&self) -> (String, String) {
        let map = self.maps.pick(self.game_input.map_id());
        (map.id.clone(), map.name.clone())
    }

    /// Re-adopt `id` if it is the one the mode is running, so an edit to it
    /// takes effect without a restart.
    fn readopt_input_map(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if id == self.game_input.map_id() {
            let map = self.maps.pick(id).clone();
            self.game_input.set_map(map, browser, commands);
        }
    }

    /// Rebuild both devices' tables from an edited store. The pad forgets what it
    /// holds, so a press begun under the old table cannot resolve against the new.
    pub fn set_bindings(&mut self, store: &Store, commands: &mut Vec<AppCommand>) {
        self.bindings = bindings::build(store, &self.key_names);
        self.store = store.clone();
        self.gamepad.reset(commands);
    }

    /// The gestures `action` answers to on the keyboard, as `bindings.toml`
    /// spells them — for naming a way out on screen rather than assuming one.
    pub fn key_gestures(&self, action: Action) -> Vec<String> {
        self.store
            .keyboard
            .iter()
            .filter(|(_, name)| name.as_str() == action.name())
            .map(|(gesture, _)| gesture.clone())
            .collect()
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
        let capturing = ui.settings.capturing();
        if capturing != self.capture.is_on() {
            self.capture
                .set(capturing, &self.gamepad.held(), Instant::now());
            self.gamepad.reset(commands);
        }

        // Game Mode routes the pad to the game while the page owns the focus; on
        // the way out everything the page holds is released, so no key sticks.
        let game_on = ui.game_mode() && ui.focus() == Focus::Page && !self.capture.is_on();
        if game_on != self.game_active {
            self.game_active = game_on;
            if game_on {
                self.gamepad.reset(commands);
            } else {
                self.game_input.release(browser, commands);
            }
        }

        // An active pad returns promptly: it drives the cursor from a held stick,
        // which produces no event to wake on, so blocking would stall the motion.
        let device_active = match self.game_active {
            true => self.game_input.is_active(),
            false => self.gamepad.is_active(),
        };
        let waited = !device_active && !self.capture.is_on();
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
        if self.capture.is_on() {
            match self.capture.tick(Instant::now()) {
                Tick::Got(captured) => push_capture(commands, captured),
                Tick::GaveUp => commands.push(AppCommand::Settings(SettingsAction::CaptureCancel)),
                Tick::Waiting => {}
            }
            return waited;
        }
        // Emit this frame's analog state as a command for the router to apply,
        // and fire any hold or repeat whose deadline just passed.
        if self.game_active {
            self.game_input.tick(commands);
        } else {
            self.gamepad.tick(commands);
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
        let withheld = pad_of(button)
            .is_some_and(|pad| self.game_input.on_pad(pad, pressed, browser, commands));
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
                // Where the pad arrives as keys, it binds as the pad it is.
                if let Some(pad) = self.keymap.pad(*kc) {
                    self.capture.on_press(pad, now)
                } else {
                    self.capture.on_key(
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
                Some(pad) => self.capture.on_release(pad, now),
                None => self.capture.on_key_release(&key_name(*kc)),
            },
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
        if self
            .game_input
            .on_key(code, key.pressed, key.repeat, browser, commands)
        {
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
            if self.game_active {
                self.game_input.on_pad(pad, key.pressed, browser, commands);
                return;
            }
            // A pad press reclaims hint badges as button combos.
            if key.pressed {
                ui.note_input_keyboard(false);
            }
            self.gamepad
                .on_pad(pad, key.pressed, &self.bindings, commands);
            return;
        }
        // Remember the input came from the keyboard so hint mode picks
        // typed-letter badges when it opens (see `AppUi::note_input_keyboard`).
        if key.pressed {
            ui.note_input_keyboard(true);
        }
        if self.game_active {
            self.game_key(&key, browser, commands);
            return;
        }
        super::keyboard::on_key(&key, &self.bindings, ui, browser, commands);
    }

    /// One pad-button edge, either direction — the controller twin of
    /// [`Self::on_key_event`].
    fn on_pad_button(
        &mut self,
        which: u32,
        button: sdl2::controller::Button,
        pressed: bool,
        ui: &mut AppUi,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if self.game_active {
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
        self.gamepad
            .on_button(button, pressed, &self.bindings, commands);
    }

    fn handle_event(
        &mut self,
        event: Event,
        window: &mut AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        if self.capture.is_on() && self.on_capture_event(&event, commands) {
            return;
        }

        // A modified key stays ours: egui reports *every* key consumed while a
        // text field has focus. Game Mode's keys belong to the page, not egui.
        let egui_first = !self.is_pad_as_keys(&event) && !(self.game_active && is_key(&event));
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
                if self.game_active {
                    if !self.game_input.on_axis(axis, value, browser, commands) {
                        self.to_page(browser, which, |slot| gamepad_api::axis(slot, axis, value));
                    }
                    return;
                }
                self.to_page(browser, which, |slot| gamepad_api::axis(slot, axis, value));
                self.gamepad.on_axis(axis, value, &self.bindings, commands);
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
