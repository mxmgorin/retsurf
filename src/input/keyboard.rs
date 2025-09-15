use crate::app::{AppCmd};
use keyboard_types::{Code, Key, KeyState, KeyboardEvent, Location, Modifiers, NamedKey};
use sdl2::keyboard::{Keycode, Mod, Scancode};

pub fn handle_keyboard(kc: Keycode, sc: Scancode, m: Mod, down: bool, repeat: bool) -> Vec<AppCmd> {
    let state = if down { KeyState::Down } else { KeyState::Up };
    let kb_event = KeyboardEvent {
        state,
        key: sdl_keycode_to_key(kc),
        code: sdl_scancode_to_code(sc),
        location: Location::Standard,
        modifiers: sdl_mod_to_modifiers(m),
        repeat,
        is_composing: false,
    };
    let event = servo::InputEvent::Keyboard(servo::KeyboardEvent::new(kb_event));

    vec![AppCmd::HandleInput(event)]
}

fn sdl_mod_to_modifiers(m: Mod) -> Modifiers {
    let mut mods = Modifiers::empty();
    if m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) {
        mods |= Modifiers::CONTROL;
    }
    if m.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD) {
        mods |= Modifiers::SHIFT;
    }
    if m.intersects(Mod::LALTMOD | Mod::RALTMOD) {
        mods |= Modifiers::ALT;
    }
    if m.intersects(Mod::LGUIMOD | Mod::RGUIMOD) {
        mods |= Modifiers::META;
    }

    mods
}

/// --- Scancode -> Code (physical key mapping) ---
fn sdl_scancode_to_code(sc: Scancode) -> Code {
    match sc {
        Scancode::A => Code::KeyA,
        Scancode::B => Code::KeyB,
        Scancode::C => Code::KeyC,
        Scancode::D => Code::KeyD,
        Scancode::E => Code::KeyE,
        Scancode::F => Code::KeyF,
        Scancode::G => Code::KeyG,
        Scancode::H => Code::KeyH,
        Scancode::I => Code::KeyI,
        Scancode::J => Code::KeyJ,
        Scancode::K => Code::KeyK,
        Scancode::L => Code::KeyL,
        Scancode::M => Code::KeyM,
        Scancode::N => Code::KeyN,
        Scancode::O => Code::KeyO,
        Scancode::P => Code::KeyP,
        Scancode::Q => Code::KeyQ,
        Scancode::R => Code::KeyR,
        Scancode::S => Code::KeyS,
        Scancode::T => Code::KeyT,
        Scancode::U => Code::KeyU,
        Scancode::V => Code::KeyV,
        Scancode::W => Code::KeyW,
        Scancode::X => Code::KeyX,
        Scancode::Y => Code::KeyY,
        Scancode::Z => Code::KeyZ,

        Scancode::Num1 => Code::Digit1,
        Scancode::Num2 => Code::Digit2,
        Scancode::Num3 => Code::Digit3,
        Scancode::Num4 => Code::Digit4,
        Scancode::Num5 => Code::Digit5,
        Scancode::Num6 => Code::Digit6,
        Scancode::Num7 => Code::Digit7,
        Scancode::Num8 => Code::Digit8,
        Scancode::Num9 => Code::Digit9,
        Scancode::Num0 => Code::Digit0,

        Scancode::Return => Code::Enter,
        Scancode::Escape => Code::Escape,
        Scancode::Backspace => Code::Backspace,
        Scancode::Tab => Code::Tab,
        Scancode::Space => Code::Space,

        Scancode::Minus => Code::Minus,
        Scancode::Equals => Code::Equal,
        Scancode::LeftBracket => Code::BracketLeft,
        Scancode::RightBracket => Code::BracketRight,
        Scancode::Backslash => Code::Backslash,
        Scancode::Semicolon => Code::Semicolon,
        Scancode::Apostrophe => Code::Quote,
        Scancode::Grave => Code::Backquote,
        Scancode::Comma => Code::Comma,
        Scancode::Period => Code::Period,
        Scancode::Slash => Code::Slash,

        Scancode::CapsLock => Code::CapsLock,

        Scancode::F1 => Code::F1,
        Scancode::F2 => Code::F2,
        Scancode::F3 => Code::F3,
        Scancode::F4 => Code::F4,
        Scancode::F5 => Code::F5,
        Scancode::F6 => Code::F6,
        Scancode::F7 => Code::F7,
        Scancode::F8 => Code::F8,
        Scancode::F9 => Code::F9,
        Scancode::F10 => Code::F10,
        Scancode::F11 => Code::F11,
        Scancode::F12 => Code::F12,

        Scancode::PrintScreen => Code::PrintScreen,
        Scancode::ScrollLock => Code::ScrollLock,
        Scancode::Pause => Code::Pause,

        Scancode::Insert => Code::Insert,
        Scancode::Home => Code::Home,
        Scancode::PageUp => Code::PageUp,
        Scancode::Delete => Code::Delete,
        Scancode::End => Code::End,
        Scancode::PageDown => Code::PageDown,

        Scancode::Right => Code::ArrowRight,
        Scancode::Left => Code::ArrowLeft,
        Scancode::Down => Code::ArrowDown,
        Scancode::Up => Code::ArrowUp,

        Scancode::NumLockClear => Code::NumLock,
        Scancode::KpDivide => Code::NumpadDivide,
        Scancode::KpMultiply => Code::NumpadMultiply,
        Scancode::KpMinus => Code::NumpadSubtract,
        Scancode::KpPlus => Code::NumpadAdd,
        Scancode::KpEnter => Code::NumpadEnter,
        Scancode::Kp1 => Code::Numpad1,
        Scancode::Kp2 => Code::Numpad2,
        Scancode::Kp3 => Code::Numpad3,
        Scancode::Kp4 => Code::Numpad4,
        Scancode::Kp5 => Code::Numpad5,
        Scancode::Kp6 => Code::Numpad6,
        Scancode::Kp7 => Code::Numpad7,
        Scancode::Kp8 => Code::Numpad8,
        Scancode::Kp9 => Code::Numpad9,
        Scancode::Kp0 => Code::Numpad0,
        Scancode::KpPeriod => Code::NumpadDecimal,

        Scancode::LCtrl => Code::ControlLeft,
        Scancode::LShift => Code::ShiftLeft,
        Scancode::LAlt => Code::AltLeft,
        Scancode::LGui => Code::MetaLeft,
        Scancode::RCtrl => Code::ControlRight,
        Scancode::RShift => Code::ShiftRight,
        Scancode::RAlt => Code::AltRight,
        Scancode::RGui => Code::MetaRight,

        _ => Code::Unidentified,
    }
}

/// --- Keycode -> Key (logical meaning) ---
fn sdl_keycode_to_key(kc: Keycode) -> Key {
    match kc {
        Keycode::A => Key::Character("a".into()),
        Keycode::B => Key::Character("b".into()),
        Keycode::C => Key::Character("c".into()),
        Keycode::D => Key::Character("d".into()),
        Keycode::E => Key::Character("e".into()),
        Keycode::F => Key::Character("f".into()),
        Keycode::G => Key::Character("g".into()),
        Keycode::H => Key::Character("h".into()),
        Keycode::I => Key::Character("i".into()),
        Keycode::J => Key::Character("j".into()),
        Keycode::K => Key::Character("k".into()),
        Keycode::L => Key::Character("l".into()),
        Keycode::M => Key::Character("m".into()),
        Keycode::N => Key::Character("n".into()),
        Keycode::O => Key::Character("o".into()),
        Keycode::P => Key::Character("p".into()),
        Keycode::Q => Key::Character("q".into()),
        Keycode::R => Key::Character("r".into()),
        Keycode::S => Key::Character("s".into()),
        Keycode::T => Key::Character("t".into()),
        Keycode::U => Key::Character("u".into()),
        Keycode::V => Key::Character("v".into()),
        Keycode::W => Key::Character("w".into()),
        Keycode::X => Key::Character("x".into()),
        Keycode::Y => Key::Character("y".into()),
        Keycode::Z => Key::Character("z".into()),

        Keycode::Num1 => Key::Character("1".into()),
        Keycode::Num2 => Key::Character("2".into()),
        Keycode::Num3 => Key::Character("3".into()),
        Keycode::Num4 => Key::Character("4".into()),
        Keycode::Num5 => Key::Character("5".into()),
        Keycode::Num6 => Key::Character("6".into()),
        Keycode::Num7 => Key::Character("7".into()),
        Keycode::Num8 => Key::Character("8".into()),
        Keycode::Num9 => Key::Character("9".into()),
        Keycode::Num0 => Key::Character("0".into()),

        Keycode::Space => Key::Character(" ".into()),
        Keycode::Return => Key::Named(NamedKey::Enter),
        Keycode::Escape => Key::Named(NamedKey::Escape),
        Keycode::Backspace => Key::Named(NamedKey::Backspace),
        Keycode::Tab => Key::Named(NamedKey::Tab),

        Keycode::Minus => Key::Character("-".into()),
        Keycode::Equals => Key::Character("=".into()),
        Keycode::LeftBracket => Key::Character("[".into()),
        Keycode::RightBracket => Key::Character("]".into()),
        Keycode::Backslash => Key::Character("\\".into()),
        Keycode::Semicolon => Key::Character(";".into()),
        Keycode::Comma => Key::Character(",".into()),
        Keycode::Period => Key::Character(".".into()),
        Keycode::Slash => Key::Character("/".into()),

        Keycode::CapsLock => Key::Named(NamedKey::CapsLock),

        Keycode::F1 => Key::Named(NamedKey::F1),
        Keycode::F2 => Key::Named(NamedKey::F2),
        Keycode::F3 => Key::Named(NamedKey::F3),
        Keycode::F4 => Key::Named(NamedKey::F4),
        Keycode::F5 => Key::Named(NamedKey::F5),
        Keycode::F6 => Key::Named(NamedKey::F6),
        Keycode::F7 => Key::Named(NamedKey::F7),
        Keycode::F8 => Key::Named(NamedKey::F8),
        Keycode::F9 => Key::Named(NamedKey::F9),
        Keycode::F10 => Key::Named(NamedKey::F10),
        Keycode::F11 => Key::Named(NamedKey::F11),
        Keycode::F12 => Key::Named(NamedKey::F12),

        Keycode::PrintScreen => Key::Named(NamedKey::PrintScreen),
        Keycode::ScrollLock => Key::Named(NamedKey::ScrollLock),
        Keycode::Pause => Key::Named(NamedKey::Pause),

        Keycode::Insert => Key::Named(NamedKey::Insert),
        Keycode::Home => Key::Named(NamedKey::Home),
        Keycode::PageUp => Key::Named(NamedKey::PageUp),
        Keycode::Delete => Key::Named(NamedKey::Delete),
        Keycode::End => Key::Named(NamedKey::End),
        Keycode::PageDown => Key::Named(NamedKey::PageDown),

        Keycode::Right => Key::Named(NamedKey::ArrowRight),
        Keycode::Left => Key::Named(NamedKey::ArrowLeft),
        Keycode::Down => Key::Named(NamedKey::ArrowDown),
        Keycode::Up => Key::Named(NamedKey::ArrowUp),

        Keycode::NumLockClear => Key::Named(NamedKey::NumLock),
        Keycode::KpDivide => Key::Character("/".into()),
        Keycode::KpMultiply => Key::Character("*".into()),
        Keycode::KpMinus => Key::Character("-".into()),
        Keycode::KpPlus => Key::Character("+".into()),
        Keycode::KpEnter => Key::Named(NamedKey::Enter),
        Keycode::Kp1 => Key::Character("1".into()),
        Keycode::Kp2 => Key::Character("2".into()),
        Keycode::Kp3 => Key::Character("3".into()),
        Keycode::Kp4 => Key::Character("4".into()),
        Keycode::Kp5 => Key::Character("5".into()),
        Keycode::Kp6 => Key::Character("6".into()),
        Keycode::Kp7 => Key::Character("7".into()),
        Keycode::Kp8 => Key::Character("8".into()),
        Keycode::Kp9 => Key::Character("9".into()),
        Keycode::Kp0 => Key::Character("0".into()),
        Keycode::KpPeriod => Key::Character(".".into()),

        Keycode::LCtrl => Key::Named(NamedKey::Control),
        Keycode::LShift => Key::Named(NamedKey::Shift),
        Keycode::LAlt => Key::Named(NamedKey::Alt),
        Keycode::LGui => Key::Named(NamedKey::Meta),
        Keycode::RCtrl => Key::Named(NamedKey::Control),
        Keycode::RShift => Key::Named(NamedKey::Shift),
        Keycode::RAlt => Key::Named(NamedKey::Alt),
        Keycode::RGui => Key::Named(NamedKey::Meta),

        _ => unimplemented!("{kc:?}"),
    }
}
