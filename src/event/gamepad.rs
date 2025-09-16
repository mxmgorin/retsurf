use sdl2::controller::Button;
use crate::app::AppCommand;

pub fn handle_gamepad(_button: Button, _is_pressed: bool) -> Option<AppCommand> {
    None
}
