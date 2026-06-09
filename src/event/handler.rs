use super::gamepad::Gamepad;
use crate::{
    app::{AppCommand, MenuAction},
    browser::AppBrowser,
    event::{user::handle_user, window::handle_window},
    ui::AppUi,
    window::AppWindow,
};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

pub struct AppEventHandler {
    event_pump: sdl2::EventPump,
    game_controllers: Vec<sdl2::controller::GameController>,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
}

impl AppEventHandler {
    pub fn new(sdl: &sdl2::Sdl) -> Result<Self, String> {
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
        })
    }

    pub fn wait(
        &mut self,
        window: &AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        gamepad: &mut Gamepad,
        commands: &mut Vec<AppCommand>,
    ) {
        // Block for the next event only when idle. When the gamepad is active or
        // the page is animating, return promptly so the main loop keeps ticking
        // (vsync caps the rate); blocking here would stall cursor/scroll motion.
        if !browser.is_animating() && !gamepad.is_active() {
            match ui.take_repain_delay() {
                Some(delay) => {
                    if let Some(event) =
                        self.event_pump.wait_event_timeout(delay.as_millis() as u32)
                    {
                        self.handle_event(event, window, ui, browser, gamepad, commands);
                    }
                }
                None => {
                    let event = self.event_pump.wait_event();
                    self.handle_event(event, window, ui, browser, gamepad, commands);
                }
            }
        }

        // Drain everything else queued this frame (notably the flood of analog
        // stick axis events) so we always act on the latest input — no backlog lag.
        while let Some(event) = self.event_pump.poll_event() {
            self.handle_event(event, window, ui, browser, gamepad, commands);
        }
    }

    fn handle_event(
        &mut self,
        event: Event,
        window: &AppWindow,
        ui: &mut AppUi,
        browser: &mut AppBrowser,
        gamepad: &mut Gamepad,
        commands: &mut Vec<AppCommand>,
    ) {
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
                let (x, y) = ui.into_browser_rel_pos(x as f32, y as f32);
                let event = super::sdl2_servo::into_mouse_button_event(mouse_btn, x, y, false);
                let event = servo::InputEvent::MouseButton(event);
                browser.handle_input(event);
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                let (x, y) = ui.into_browser_rel_pos(x as f32, y as f32);
                let event = super::sdl2_servo::into_mouse_button_event(mouse_btn, x, y, true);
                let event = servo::InputEvent::MouseButton(event);

                browser.handle_input(event);
            }
            Event::MouseMotion { x, y, .. } => {
                let (x, y) = ui.into_browser_rel_pos(x as f32, y as f32);
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
                let (mx, my) = ui.into_browser_rel_pos(mouse_x as f32, mouse_y as f32);
                // Fire the DOM `wheel` event (for pages with JS handlers)...
                let event = super::sdl2_servo::into_wheel_event(x, y, mx, my);
                browser.handle_input(servo::InputEvent::Wheel(event));
                // ...then perform the actual native scroll. SDL `y` is positive
                // when scrolling up; Servo's positive `dy` reveals lower content.
                const WHEEL_PX: f32 = 60.0;
                browser.scroll(-x as f32 * WHEEL_PX, -y as f32 * WHEEL_PX, mx, my);
            }
            // While the menu is open it captures the keyboard: Esc closes, Enter
            // opens, Delete removes, up/down move the selection, left/right switch
            // section.
            Event::KeyDown { keycode: Some(kc), .. } if ui.menu_visible() => {
                match kc {
                    Keycode::Escape => commands.push(AppCommand::Menu(MenuAction::Close)),
                    Keycode::Return | Keycode::KpEnter => {
                        commands.push(AppCommand::Menu(MenuAction::OpenSelected))
                    }
                    Keycode::Delete | Keycode::Backspace => {
                        commands.push(AppCommand::Menu(MenuAction::RemoveSelected))
                    }
                    Keycode::Up => commands.push(AppCommand::Menu(MenuAction::Move(-1))),
                    Keycode::Down => commands.push(AppCommand::Menu(MenuAction::Move(1))),
                    Keycode::Left => commands.push(AppCommand::Menu(MenuAction::SwitchSection(-1))),
                    Keycode::Right => commands.push(AppCommand::Menu(MenuAction::SwitchSection(1))),
                    _ => {}
                }
            }
            Event::KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let event = super::sdl2_servo::into_keyboard_event(kc, sc, keymod, true, repeat);
                let event = servo::InputEvent::Keyboard(event);
                browser.handle_input(event);
            }
            // Swallow key releases too while the menu owns the keyboard.
            Event::KeyUp { .. } if ui.menu_visible() => {}
            Event::KeyUp {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let event = super::sdl2_servo::into_keyboard_event(kc, sc, keymod, false, repeat);
                let event = servo::InputEvent::Keyboard(event);
                browser.handle_input(event);
            }
            Event::ControllerAxisMotion { axis, value, .. } => {
                gamepad.on_axis(axis, value, commands);
            }
            Event::ControllerButtonDown { button, .. } => {
                gamepad.on_button(button, true, commands);
            }
            Event::ControllerButtonUp { button, .. } => {
                gamepad.on_button(button, false, commands);
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
