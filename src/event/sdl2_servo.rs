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
        key: sdl2_keyboard_types::into_key(kc),
        code: sdl2_keyboard_types::into_code(sc),
        location: keyboard_types::Location::Standard,
        modifiers: sdl2_keyboard_types::into_modifiers(m),
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
        sdl2::mouse::MouseButton::Left => servo::MouseButton::Primary,
        sdl2::mouse::MouseButton::Middle => servo::MouseButton::Auxiliary,
        sdl2::mouse::MouseButton::Right => servo::MouseButton::Secondary,
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
fn into_device_point(x: f32, y: f32) -> servo::WebViewPoint {
    servo::DevicePoint::new(x, y).into()
}

/// A keyboard event for a printable character, for on-screen-keyboard and
/// game-mode input.
pub fn char_keyboard_event(c: char, shift: bool, down: bool) -> servo::KeyboardEvent {
    let state = if down {
        keyboard_types::KeyState::Down
    } else {
        keyboard_types::KeyState::Up
    };
    let modifiers = if shift {
        keyboard_types::Modifiers::SHIFT
    } else {
        keyboard_types::Modifiers::empty()
    };
    let event = keyboard_types::KeyboardEvent {
        state,
        key: keyboard_types::Key::Character(c.to_string()),
        code: code_for_char(c),
        location: keyboard_types::Location::Standard,
        modifiers,
        repeat: false,
        is_composing: false,
    };
    servo::KeyboardEvent::new(event)
}

/// The `code` for a printable character where the standard defines one; games
/// branch on `e.code` (`KeyW` for WASD) for layout-independent input.
fn code_for_char(c: char) -> keyboard_types::Code {
    use keyboard_types::Code;
    match c.to_ascii_lowercase() {
        'a' => Code::KeyA,
        'b' => Code::KeyB,
        'c' => Code::KeyC,
        'd' => Code::KeyD,
        'e' => Code::KeyE,
        'f' => Code::KeyF,
        'g' => Code::KeyG,
        'h' => Code::KeyH,
        'i' => Code::KeyI,
        'j' => Code::KeyJ,
        'k' => Code::KeyK,
        'l' => Code::KeyL,
        'm' => Code::KeyM,
        'n' => Code::KeyN,
        'o' => Code::KeyO,
        'p' => Code::KeyP,
        'q' => Code::KeyQ,
        'r' => Code::KeyR,
        's' => Code::KeyS,
        't' => Code::KeyT,
        'u' => Code::KeyU,
        'v' => Code::KeyV,
        'w' => Code::KeyW,
        'x' => Code::KeyX,
        'y' => Code::KeyY,
        'z' => Code::KeyZ,
        '0' => Code::Digit0,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        '9' => Code::Digit9,
        ' ' => Code::Space,
        _ => Code::Unidentified,
    }
}

/// A keyboard event for a named key (Enter, Backspace, …).
pub fn named_keyboard_event(
    key: keyboard_types::NamedKey,
    code: keyboard_types::Code,
    down: bool,
) -> servo::KeyboardEvent {
    let state = if down {
        keyboard_types::KeyState::Down
    } else {
        keyboard_types::KeyState::Up
    };
    let event = keyboard_types::KeyboardEvent {
        state,
        key: keyboard_types::Key::Named(key),
        code,
        location: keyboard_types::Location::Standard,
        modifiers: keyboard_types::Modifiers::empty(),
        repeat: false,
        is_composing: false,
    };
    servo::KeyboardEvent::new(event)
}

#[cfg(test)]
mod tests {
    use super::code_for_char;
    use keyboard_types::Code;

    #[test]
    fn a_character_carries_its_code_where_the_standard_has_one() {
        assert_eq!(code_for_char('w'), Code::KeyW);
        assert_eq!(code_for_char('W'), Code::KeyW);
        assert_eq!(code_for_char('5'), Code::Digit5);
        assert_eq!(code_for_char(' '), Code::Space);
        assert_eq!(code_for_char('?'), Code::Unidentified);
    }
}
