//! The page's view of the pad: SDL controller input in the Standard Gamepad
//! mapping the Gamepad API defines.
//!
//! Separate from [`super::gamepad`], which resolves the same input into chrome
//! actions. The API is polled (`navigator.getGamepads()`), so it consumes
//! nothing — a button can drive the chrome and reach the page in the same press.

use sdl2::controller::{Axis, Button};
use servo::{
    GamepadEvent, GamepadIndex, GamepadInputBounds, GamepadSupportedHapticEffects,
    GamepadUpdateType,
};

/// Full scale of an SDL analog axis, so `-1.0..=1.0` for a stick and
/// `0.0..=1.0` for a trigger (SDL reports those from zero up).
const AXIS_MAX: f64 = 32767.0;

/// The bounds every pad here reports: SDL normalizes each device to this range,
/// which is also what the Standard Gamepad mapping is defined in.
const BOUNDS: GamepadInputBounds = GamepadInputBounds {
    axis_bounds: (-1.0, 1.0),
    button_bounds: (0.0, 1.0),
};

pub fn connected(slot: usize, name: String) -> GamepadEvent {
    GamepadEvent::Connected(
        GamepadIndex(slot),
        name,
        BOUNDS,
        // SDL exposes rumble on both, and reports failure per call rather than
        // up front; a pad without motors simply does nothing.
        GamepadSupportedHapticEffects {
            supports_dual_rumble: true,
            supports_trigger_rumble: true,
        },
    )
}

pub fn disconnected(slot: usize) -> GamepadEvent {
    GamepadEvent::Disconnected(GamepadIndex(slot))
}

/// A digital button edge. `None` for a button outside the standard mapping.
pub fn button(slot: usize, button: Button, pressed: bool) -> Option<GamepadEvent> {
    let value = if pressed { 1.0 } else { 0.0 };
    Some(GamepadEvent::Updated(
        GamepadIndex(slot),
        GamepadUpdateType::Button(standard_button(button)?, value),
    ))
}

/// A stick or trigger. Triggers are buttons 6 and 7 in the standard mapping,
/// analog rather than an edge, which is why they arrive here.
pub fn axis(slot: usize, axis: Axis, value: i16) -> Option<GamepadEvent> {
    let update = match axis {
        Axis::TriggerLeft => GamepadUpdateType::Button(6, value as f64 / AXIS_MAX),
        Axis::TriggerRight => GamepadUpdateType::Button(7, value as f64 / AXIS_MAX),
        // SDL and the spec agree on the sign: right and down are positive.
        Axis::LeftX => GamepadUpdateType::Axis(0, value as f64 / AXIS_MAX),
        Axis::LeftY => GamepadUpdateType::Axis(1, value as f64 / AXIS_MAX),
        Axis::RightX => GamepadUpdateType::Axis(2, value as f64 / AXIS_MAX),
        Axis::RightY => GamepadUpdateType::Axis(3, value as f64 / AXIS_MAX),
    };
    Some(GamepadEvent::Updated(GamepadIndex(slot), update))
}

/// <https://www.w3.org/TR/gamepad/#dfn-represents-a-standard-gamepad-button>
fn standard_button(button: Button) -> Option<usize> {
    Some(match button {
        Button::A => 0,
        Button::B => 1,
        Button::X => 2,
        Button::Y => 3,
        Button::LeftShoulder => 4,
        Button::RightShoulder => 5,
        // 6 and 7 are the triggers, which SDL reports as axes.
        Button::Back => 8,
        Button::Start => 9,
        Button::LeftStick => 10,
        Button::RightStick => 11,
        Button::DPadUp => 12,
        Button::DPadDown => 13,
        Button::DPadLeft => 14,
        Button::DPadRight => 15,
        Button::Guide => 16,
        // Paddles, touchpad and the misc buttons have no standard index.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triggers_are_buttons_and_the_sticks_keep_their_sign() {
        let trigger = axis(0, Axis::TriggerRight, 32767).unwrap();
        assert!(matches!(
            trigger,
            GamepadEvent::Updated(GamepadIndex(0), GamepadUpdateType::Button(7, v)) if v == 1.0
        ));
        let down = axis(0, Axis::LeftY, 32767).unwrap();
        assert!(matches!(
            down,
            GamepadEvent::Updated(GamepadIndex(0), GamepadUpdateType::Axis(1, v)) if v == 1.0
        ));
    }

    #[test]
    fn the_face_buttons_follow_the_standard_order() {
        assert_eq!(standard_button(Button::A), Some(0));
        assert_eq!(standard_button(Button::Y), Some(3));
        assert_eq!(standard_button(Button::DPadRight), Some(15));
        assert_eq!(standard_button(Button::Touchpad), None);
    }
}
