use sdl2::keyboard::Keycode;
use crate::app::AppCmd;

pub fn handle_keyboard(_keycode: Keycode, _is_pressed: bool) -> Option<AppCmd> {
    None
}
