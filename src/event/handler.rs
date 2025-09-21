use super::{gamepad::handle_gamepad, keyboard::into_servo_keyboard};
use crate::{
    app::AppCommand,
    event::{
        mouse::{into_servo_mouse_button, into_servo_mouse_move, into_servo_mouse_wheel},
        user::handle_user,
        window::handle_window,
    },
    ui::AppUi,
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

    pub fn wait(&mut self, ui: &mut AppUi) -> Vec<AppCommand> {
        let mut commands_buffer = Vec::with_capacity(2);
        let delay = ui.take_repain_delay();
        let event = if let Some(delay) = delay {
            if let Some(event) = self.event_pump.wait_event_timeout(delay.as_millis() as u32) {
                // todo: we will skip draw when there is event faster then delay
                event
            } else {
                commands_buffer.push(AppCommand::Draw);
                return commands_buffer;
            }
        } else {
            self.event_pump.wait_event()
        };

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
                let input = into_servo_mouse_button(mouse_btn, x, y, false);
                commands_buffer.push(AppCommand::HandleInput(input));
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                let input = into_servo_mouse_button(mouse_btn, x, y, true);
                commands_buffer.push(AppCommand::HandleInput(input));
            }
            Event::MouseMotion { x, y, .. } => {
                let input = into_servo_mouse_move(x, y);
                commands_buffer.push(AppCommand::HandleInput(input));
            }
            Event::MouseWheel {
                precise_x,
                precise_y,
                mouse_x,
                mouse_y,
                ..
            } => {
                let input = into_servo_mouse_wheel(precise_x, precise_y, mouse_x, mouse_y);
                commands_buffer.push(AppCommand::HandleInput(input));
            }
            Event::KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let input = into_servo_keyboard(kc, sc, keymod, true, repeat);
                commands_buffer.push(AppCommand::HandleInput(input));
            }
            Event::KeyUp {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let input = into_servo_keyboard(kc, sc, keymod, false, repeat);
                commands_buffer.push(AppCommand::HandleInput(input));
            }
            Event::ControllerButtonDown { button, .. } => {
                if let Some(cmd) = handle_gamepad(button, true) {
                    commands_buffer.push(cmd);
                }
            }
            Event::ControllerButtonUp { button, .. } => {
                if let Some(cmd) = handle_gamepad(button, false) {
                    commands_buffer.push(cmd);
                }
            }
            Event::Quit { .. } => commands_buffer.push(AppCommand::Shutdown),
            Event::User { code, .. } => {
                if let Some(cmd) = handle_user(code) {
                    commands_buffer.push(cmd);
                }
            }
            Event::Window { win_event, .. } => {
                if let Some(cmd) = handle_window(win_event) {
                    commands_buffer.push(cmd);
                }
            }
            _ => {}
        }

        commands_buffer
    }
}
