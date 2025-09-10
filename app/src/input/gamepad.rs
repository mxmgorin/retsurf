use sdl2::controller::Button;
use crate::app::AppCmd;

pub fn handle_gamepad(_button: Button, _is_pressed: bool) -> Option<AppCmd> {
    None
}
