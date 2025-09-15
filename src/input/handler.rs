use super::{gamepad::handle_gamepad, keyboard::handle_keyboard};
use crate::{
    app::AppCmd,
    input::{
        mouse::{handle_mouse_button, handle_mouse_move, handle_mouse_wheel},
        user::handle_user,
    },
};
use sdl2::event::Event;

pub struct AppInputHandler {
    event_pump: sdl2::EventPump,
    game_controllers: Vec<sdl2::controller::GameController>,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
}

impl AppInputHandler {
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

    pub fn wait_event(&mut self) -> Vec<AppCmd> {
        let event = self.event_pump.wait_event();

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
            } => return handle_mouse_button(mouse_btn, x, y, false),
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => return handle_mouse_button(mouse_btn, x, y, true),
            Event::MouseMotion { x, y, .. } => return handle_mouse_move(x, y),
            Event::MouseWheel {
                precise_x,
                precise_y,
                mouse_x,
                mouse_y,
                ..
            } => return handle_mouse_wheel(precise_x, precise_y, mouse_x, mouse_y),
            Event::KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => return handle_keyboard(kc, sc, keymod, true, repeat),
            Event::KeyUp {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => return handle_keyboard(kc, sc, keymod, false, repeat),
            Event::ControllerButtonDown { button, .. } => {
                if let Some(cmd) = handle_gamepad(button, true) {
                    return vec![cmd];
                }
            }
            Event::ControllerButtonUp { button, .. } => {
                if let Some(cmd) = handle_gamepad(button, false) {
                    return vec![cmd];
                }
            }
            Event::Quit { .. } => return vec![AppCmd::Quit],
            Event::User { code, .. } => {
                if let Some(cmd) = handle_user(code) {
                    return vec![cmd];
                }
            }
            Event::Window {
                win_event: sdl2::event::WindowEvent::Close,
                ..
            } => {
                return vec![AppCmd::Quit];
            }
            _ => {}
        }

        vec![]
    }
}
