pub fn into_keyboard_event(
    kc: sdl2::keyboard::Keycode,
    sc: sdl2::keyboard::Scancode,
    m: sdl2::keyboard::Mod,
    down: bool,
    repeat: bool,
) -> servo::KeyboardEvent {
    let state = if down {
        keyboard_types::KeyState::Down
    } else {
        keyboard_types::KeyState::Up
    };
    let event = keyboard_types::KeyboardEvent {
        state,
        key: super::sdl2_keyboard_types::into_key(kc),
        code: super::sdl2_keyboard_types::into_code(sc),
        location: keyboard_types::Location::Standard,
        modifiers: super::sdl2_keyboard_types::into_modifiers(m),
        repeat,
        is_composing: false,
    };

    servo::KeyboardEvent::new(event)
}

pub fn into_mouse_button_event(
    button: sdl2::mouse::MouseButton,
    x: f32,
    y: f32,
    down: bool,
) -> servo::MouseButtonEvent {
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
    servo::MouseButtonEvent::new(action, button, point)
}

pub fn into_mouse_move_event(x: f32, y: f32) -> servo::MouseMoveEvent {
    let point = into_device_point(x, y);
    servo::MouseMoveEvent::new(point)
}

pub fn into_wheel_event(dx: i32, dy: i32, mouse_x: f32, mouse_y: f32) -> servo::WheelEvent {
    let delta = servo::WheelDelta {
        x: dx as f64,
        y: dy as f64,
        z: 0.0,
        mode: servo::WheelMode::DeltaLine,
    };
    let point = into_device_point(mouse_x, mouse_y);
    servo::WheelEvent::new(delta, point)
}

#[inline]
fn into_device_point(x: f32, y: f32) -> servo::webrender_api::units::DevicePoint {
    servo::webrender_api::units::DevicePoint::new(x, y)
}
