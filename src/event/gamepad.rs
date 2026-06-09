use crate::app::{AppCommand, InputCommand, MenuAction};
use crate::config::GamepadConfig;
use crate::osk::OskCommand;
use sdl2::controller::{Axis, Button};

/// `i16::MAX`, the full-scale value SDL reports for a stick/trigger axis.
const AXIS_MAX: f32 = 32767.0;

/// Translates raw controller input into [`InputCommand`]s — and nothing more. It
/// holds device state (stick/D-pad/trigger positions) and maps physical controls
/// to intents with a flat lookup; *what* each intent does, and where it goes, is
/// decided by the central router (`App::route_input`). On a handheld the pad is
/// the only input device, so this is the primary UI.
pub struct Gamepad {
    /// Left stick vector, normalized and dead-zoned (-1..=1).
    left: (f32, f32),
    /// D-pad vector (digital -1/0/1), combined with the stick into the aim vector.
    dpad: (f32, f32),
    /// Right stick vector, normalized and dead-zoned (-1..=1).
    right: (f32, f32),
    /// Latched L2/R2 trigger states, for press-edge detection.
    l2_down: bool,
    r2_down: bool,
    /// Tunables (dead zones, trigger threshold) loaded from the config file.
    cfg: GamepadConfig,
}

impl Gamepad {
    pub fn new(cfg: GamepadConfig) -> Self {
        Self {
            left: (0.0, 0.0),
            dpad: (0.0, 0.0),
            right: (0.0, 0.0),
            l2_down: false,
            r2_down: false,
            cfg,
        }
    }

    /// Combined aim vector (left stick + D-pad), clamped to -1..=1.
    fn aim(&self) -> (f32, f32) {
        (
            (self.left.0 + self.dpad.0).clamp(-1.0, 1.0),
            (self.left.1 + self.dpad.1).clamp(-1.0, 1.0),
        )
    }

    /// Whether the loop should keep ticking at ~60fps to animate cursor/scroll.
    pub fn is_active(&self) -> bool {
        self.aim() != (0.0, 0.0) || self.right.1 != 0.0
    }

    pub fn on_axis(&mut self, axis: Axis, value: i16, commands: &mut Vec<AppCommand>) {
        // L2/R2 are throttle-style axes, emitted on both edges as a contextual
        // trigger intent: the router uses them for the on-screen keyboard (L2 Shift,
        // R2 Enter) when it's open, otherwise to cycle tabs (L2 previous, R2 next).
        if matches!(axis, Axis::TriggerLeft | Axis::TriggerRight) {
            let pressed = value as f32 / AXIS_MAX > self.cfg.trigger_threshold;
            match axis {
                Axis::TriggerLeft if pressed != self.l2_down => {
                    self.l2_down = pressed;
                    commands.push(AppCommand::Input(InputCommand::Trigger {
                        right: false,
                        pressed,
                    }));
                }
                Axis::TriggerRight if pressed != self.r2_down => {
                    self.r2_down = pressed;
                    commands.push(AppCommand::Input(InputCommand::Trigger {
                        right: true,
                        pressed,
                    }));
                }
                _ => {}
            }
            return;
        }

        let v = value as f32 / AXIS_MAX;
        let v = if v.abs() < self.cfg.deadzone { 0.0 } else { v };
        match axis {
            Axis::LeftX => self.left.0 = v,
            Axis::LeftY => self.left.1 = v,
            Axis::RightX => self.right.0 = v,
            Axis::RightY => self.right.1 = v,
            _ => {}
        }
    }

    /// Map a controller button to a command — a flat translation with no state
    /// branches. The D-pad just feeds the aim vector (see [`Gamepad::tick`]); the
    /// contextual A/B/X become intents the router resolves.
    pub fn on_button(&mut self, button: Button, pressed: bool, commands: &mut Vec<AppCommand>) {
        let cmd = match button {
            // The D-pad contributes to the aim vector on both edges.
            Button::DPadLeft => return self.dpad.0 = if pressed { -1.0 } else { 0.0 },
            Button::DPadRight => return self.dpad.0 = if pressed { 1.0 } else { 0.0 },
            Button::DPadUp => return self.dpad.1 = if pressed { -1.0 } else { 0.0 },
            Button::DPadDown => return self.dpad.1 = if pressed { 1.0 } else { 0.0 },
            // A clicks, so it needs both edges; everything else fires on press.
            Button::A => AppCommand::Input(InputCommand::Primary(pressed)),
            _ if !pressed => return,
            Button::B => AppCommand::Input(InputCommand::Cancel),
            Button::X => AppCommand::Input(InputCommand::Keyboard),
            // Y is "space" on the open keyboard; the router reloads the page with
            // it otherwise.
            Button::Y => AppCommand::Input(InputCommand::Osk(OskCommand::Space)),
            // Contextual: switch menu sections when the menu is open, else back/forward.
            Button::LeftShoulder => AppCommand::Input(InputCommand::Shoulder(-1)),
            Button::RightShoulder => AppCommand::Input(InputCommand::Shoulder(1)),
            Button::Start => AppCommand::ToggleBookmark,
            Button::Back => AppCommand::Menu(MenuAction::Open),
            _ => return,
        };
        commands.push(cmd);
    }

    /// Emit this frame's analog state for the router to apply.
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        commands.push(AppCommand::Input(InputCommand::Analog {
            aim: self.aim(),
            scroll: self.right.1,
        }));
    }
}
