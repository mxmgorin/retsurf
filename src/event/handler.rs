use super::{gamepad::handle_gamepad, keyboard::into_servo_keyboard};
use crate::{
    app::AppCommand,
    event::{
        mouse::{into_servo_mouse_button, into_servo_mouse_move, into_servo_mouse_wheel},
        user::handle_user,
        window::handle_window,
    },
    ui::AppUi,
    window::AppWindow,
};
use sdl2::event::Event;

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

    pub fn wait(&mut self, window: &AppWindow, ui: &mut AppUi) -> Vec<AppCommand> {
        let mut commands = Vec::with_capacity(2);
        let delay = ui.take_repain_delay();
        let event = if let Some(delay) = delay {
            if let Some(event) = self.event_pump.wait_event_timeout(delay.as_millis() as u32) {
                // todo: we will skip draw when there is event faster then delay
                event
            } else {
                commands.push(AppCommand::Draw);
                return commands;
            }
        } else {
            self.event_pump.wait_event()
        };

        let resp = ui.handle_event(window, &event);

        if resp.repaint {
            commands.push(AppCommand::Draw);
        }

        if resp.consumed {
            return commands;
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
                let input =
                    into_servo_mouse_button(mouse_btn, x, y, false, ui.get_top_bar_height());
                commands.push(AppCommand::HandleInput(input));
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                let input = into_servo_mouse_button(mouse_btn, x, y, true, ui.get_top_bar_height());
                commands.push(AppCommand::HandleInput(input));
            }
            Event::MouseMotion { x, y, .. } => {
                let input = into_servo_mouse_move(x, y, ui.get_top_bar_height());
                commands.push(AppCommand::HandleInput(input));
            }
            Event::MouseWheel {
                precise_x,
                precise_y,
                mouse_x,
                mouse_y,
                ..
            } => {
                let input = into_servo_mouse_wheel(
                    precise_x,
                    precise_y,
                    mouse_x,
                    mouse_y,
                    ui.get_top_bar_height(),
                );
                commands.push(AppCommand::HandleInput(input));
            }
            Event::KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let input = into_servo_keyboard(kc, sc, keymod, true, repeat);
                commands.push(AppCommand::HandleInput(input));
            }
            Event::KeyUp {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let input = into_servo_keyboard(kc, sc, keymod, false, repeat);
                commands.push(AppCommand::HandleInput(input));
            }
            Event::ControllerButtonDown { button, .. } => {
                if let Some(cmd) = handle_gamepad(button, true) {
                    commands.push(cmd);
                }
            }
            Event::ControllerButtonUp { button, .. } => {
                if let Some(cmd) = handle_gamepad(button, false) {
                    commands.push(cmd);
                }
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

        commands
    }
}
