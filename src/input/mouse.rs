use crate::app::{AppCmd};
use servo::{webrender_api::units::DevicePoint, InputEvent, MouseButtonEvent};

pub fn handle_mouse_button(
    button: sdl2::mouse::MouseButton,
    x: i32,
    y: i32,
    down: bool,
) -> Vec<AppCmd> {
    let action = if down {
        servo::MouseButtonAction::Down
    } else {
        servo::MouseButtonAction::Up
    };
    let button = match button {
        sdl2::mouse::MouseButton::Unknown => servo::MouseButton::Other(0),
        sdl2::mouse::MouseButton::Left => servo::MouseButton::Left,
        sdl2::mouse::MouseButton::Middle => servo::MouseButton::Middle,
        sdl2::mouse::MouseButton::Right => servo::MouseButton::Right,
        sdl2::mouse::MouseButton::X1 => servo::MouseButton::Back,
        sdl2::mouse::MouseButton::X2 => servo::MouseButton::Forward,
    };
    let point = into_device_point(x, y);
    let event = MouseButtonEvent::new(action, button, point);
    let input = InputEvent::MouseButton(event);

    vec![AppCmd::HandleInput(input)]
}

pub fn handle_mouse_move(x: i32, y: i32) -> Vec<AppCmd> {
    let point = into_device_point(x, y);
    let event = servo::MouseMoveEvent::new(point);
    let input = InputEvent::MouseMove(event);

    vec![AppCmd::HandleInput(input)]
}

fn into_device_point(x: i32, y: i32) -> DevicePoint {
    DevicePoint::new(x as f32, y as f32)
}
