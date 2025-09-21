use servo::{
    webrender_api::units::DevicePoint, InputEvent, MouseButtonEvent, WheelDelta, WheelEvent,
};

pub fn into_servo_mouse_button(
    button: sdl2::mouse::MouseButton,
    x: i32,
    y: i32,
    down: bool,
    offset_y: f32,
) -> InputEvent {
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
    let point = into_device_point(x, y, offset_y);
    let event = MouseButtonEvent::new(action, button, point);

    InputEvent::MouseButton(event)
}

pub fn into_servo_mouse_move(x: i32, y: i32, offset_y: f32) -> InputEvent {
    let point = into_device_point(x, y, offset_y);
    let event = servo::MouseMoveEvent::new(point);

    InputEvent::MouseMove(event)
}

pub fn into_servo_mouse_wheel(dx: f32, dy: f32, mouse_x: i32, mouse_y: i32, offset_y: f32) -> InputEvent {
    let delta = WheelDelta {
        x: dx as f64,
        y: dy as f64,
        z: 0.0,
        mode: servo::WheelMode::DeltaLine,
    };
    let point = into_device_point(mouse_x, mouse_y, offset_y);
    let event = WheelEvent::new(delta, point);

    InputEvent::Wheel(event)
}

fn into_device_point(x: i32, y: i32, offset_y: f32) -> DevicePoint {
    let y = y as f32 - offset_y;
    DevicePoint::new(x as f32, y as f32)
}
