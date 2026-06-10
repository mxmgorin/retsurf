//! Translates raw controller input into [`AppCommand`]s — and nothing more. It
//! holds device state (stick/D-pad/trigger positions, pressed buttons) and maps
//! physical controls to intents via the configurable [`Bindings`] table; *what*
//! each intent does is decided by the central router (`App::route_input`). On a
//! handheld the pad is the only input device, so this is the primary UI.
//!
//! Gesture resolution: a button whose only binding is a tap fires immediately
//! on the press edge, exactly like the old hardcoded layout. A button that also
//! carries a `hold:` binding or takes part in a chord is ambiguous on press, so
//! it's *deferred*: tap fires on release (if it comes before `hold_ms`), the
//! hold action fires once the threshold passes, and a chord fires the moment
//! its second button goes down (consuming both presses).
//!
//! The one action handled here rather than routed is [`Action::Scroll`]: it
//! latches `scroll_mode`, which turns the aim vector (D-pad / left stick) into
//! page scrolling — the fallback for devices without a right analog stick.

use crate::app::{AppCommand, InputCommand};
use crate::config::GamepadConfig;
use crate::event::bindings::{Action, Bindings};
use sdl2::controller::{Axis, Button};
use std::time::{Duration, Instant};

/// `i16::MAX`, the full-scale value SDL reports for a stick/trigger axis.
const AXIS_MAX: f32 = 32767.0;

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
    /// Gesture → action tables loaded from `bindings.toml`.
    bindings: Bindings,
    /// The `hold:` gesture threshold.
    hold: Duration,
    /// While latched (the `scroll` action toggles it), the aim vector scrolls
    /// the page instead of moving the cursor.
    scroll_mode: bool,
    /// Buttons currently down, with their gesture-resolution state.
    held: Vec<Held>,
}

/// One pressed button awaiting (or past) gesture resolution.
struct Held {
    button: Button,
    at: Instant,
    /// The gesture was decided — tap already dispatched on press, a chord
    /// consumed this press, or the hold fired. Release is then a no-op (except
    /// the confirm button's release edge).
    resolved: bool,
}

impl Gamepad {
    pub fn new(cfg: GamepadConfig) -> Self {
        Self {
            left: (0.0, 0.0),
            dpad: (0.0, 0.0),
            right: (0.0, 0.0),
            l2_down: false,
            r2_down: false,
            bindings: Bindings::load(),
            hold: Duration::from_millis(cfg.hold_ms),
            scroll_mode: false,
            held: vec![],
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

    /// Whether the loop should keep ticking at ~60fps: to animate cursor/scroll,
    /// and to time pending `hold:` gestures (no SDL event marks the threshold).
    pub fn is_active(&self) -> bool {
        self.aim() != (0.0, 0.0)
            || self.right.1 != 0.0
            || self
                .held
                .iter()
                .any(|h| !h.resolved && self.bindings.hold(h.button).is_some())
    }

    /// Dispatch a resolved gesture. The scroll toggle is gamepad-internal (it
    /// changes how the aim vector is read); everything else becomes a command
    /// for the router.
    fn emit(&mut self, action: Action, pressed: bool, commands: &mut Vec<AppCommand>) {
        if action == Action::Scroll {
            self.scroll_mode = !self.scroll_mode;
            return;
        }
        commands.extend(action.command(pressed));
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

    /// Resolve a button edge against the binding tables (see the module docs
    /// for the tap / hold / chord rules).
    pub fn on_button(&mut self, button: Button, pressed: bool, commands: &mut Vec<AppCommand>) {
        match button {
            // The D-pad contributes to the aim vector on both edges.
            Button::DPadLeft => return self.dpad.0 = if pressed { -1.0 } else { 0.0 },
            Button::DPadRight => return self.dpad.0 = if pressed { 1.0 } else { 0.0 },
            Button::DPadUp => return self.dpad.1 = if pressed { -1.0 } else { 0.0 },
            Button::DPadDown => return self.dpad.1 = if pressed { 1.0 } else { 0.0 },
            _ => {}
        }

        if pressed {
            self.on_press(button, commands);
        } else {
            self.on_release(button, commands);
        }
    }

    fn on_press(&mut self, button: Button, commands: &mut Vec<AppCommand>) {
        // A chord resolves the moment its second button goes down, consuming
        // both presses (neither fires its own tap on release).
        if let Some(i) = self
            .held
            .iter()
            .position(|h| !h.resolved && self.bindings.chord(h.button, button).is_some())
        {
            let action = self
                .bindings
                .chord(self.held[i].button, button)
                .expect("position() just matched");
            self.held[i].resolved = true;
            self.held.push(Held {
                button,
                at: Instant::now(),
                resolved: true,
            });
            self.emit(action, true, commands);
            return;
        }

        // Unambiguous buttons dispatch on the press edge (zero latency);
        // hold/chord candidates wait — for release, the threshold, or a chord.
        let immediate = !self.bindings.is_deferred(button);
        self.held.push(Held {
            button,
            at: Instant::now(),
            resolved: immediate,
        });
        if immediate {
            if let Some(action) = self.bindings.tap(button) {
                self.emit(action, true, commands);
            }
        }
    }

    fn on_release(&mut self, button: Button, commands: &mut Vec<AppCommand>) {
        let Some(i) = self.held.iter().position(|h| h.button == button) else {
            return; // e.g. pressed before startup
        };
        let held = self.held.remove(i);
        let action = self.bindings.tap(button);

        if held.resolved {
            // Confirm carries the press/release pair (clicks, drags): its
            // release edge always follows the press edge sent in `on_press`.
            if action == Some(Action::Confirm) {
                self.emit(Action::Confirm, false, commands);
            }
            return;
        }

        // A deferred button released before the hold threshold is a tap. Past
        // the threshold the hold action fires here as a fallback in case no
        // tick ran in between (the loop may have been blocked on this event).
        let gesture = if held.at.elapsed() >= self.hold {
            self.bindings.hold(button).or(action)
        } else {
            action
        };
        if let Some(action) = gesture {
            self.emit(action, true, commands);
        }
    }

    /// Emit this frame's analog state for the router to apply, and fire any
    /// `hold:` gesture whose threshold just passed. The aim vector is sent raw
    /// along with the latched scroll-mode flag — what it means in the current
    /// context (cursor, overlay navigation, or page scroll) is the router's
    /// decision.
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        let mut fired = vec![];
        for h in &mut self.held {
            if !h.resolved && h.at.elapsed() >= self.hold {
                if let Some(action) = self.bindings.hold(h.button) {
                    h.resolved = true;
                    fired.push(action);
                }
            }
        }
        for action in fired {
            self.emit(action, true, commands);
        }

        commands.push(AppCommand::Input(InputCommand::Analog {
            aim: self.aim(),
            scroll: self.right.1,
            scroll_mode: self.scroll_mode,
        }));
    }
}
