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

use crate::app::{AppCommand, InputCommand, SettingsAction};
use crate::config::InputConfig;
use crate::event::bindings::{self, Action, Bindings};
use sdl2::controller::{Axis, Button};
use std::time::{Duration, Instant};

/// `i16::MAX`, the full-scale value SDL reports for a stick/trigger axis.
const AXIS_MAX: f32 = 32767.0;

/// Auto-cancel binding capture if nothing is pressed for this long — the handheld
/// escape hatch (there's no Esc), so an accidental "add" doesn't trap input.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);

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
    cfg: InputConfig,
    /// Gesture → action tables loaded from `bindings.toml`.
    bindings: Bindings,
    /// The `hold:` gesture threshold.
    hold: Duration,
    /// While latched (the `scroll` action toggles it), the aim vector scrolls
    /// the page instead of moving the cursor.
    scroll_mode: bool,
    /// Buttons currently down, with their gesture-resolution state.
    held: Vec<Held>,
    /// Binding-capture mode (the settings overlay listening for a gamepad
    /// gesture). While set, normal dispatch / cursor motion is suppressed and the
    /// resolved gesture is reported as a [`SettingsAction::CaptureBinding`].
    capture: Capture,
}

/// State for binding-capture mode (see [`Gamepad::set_capture`]). Resolves the
/// same tap / hold / chord gestures as normal play, but reports the gesture
/// string instead of dispatching the bound action.
#[derive(Default)]
struct Capture {
    /// Whether capture is active.
    on: bool,
    /// Armed once the buttons held when capture began (the activating press)
    /// have all been released — so that press isn't itself captured.
    armed: bool,
    /// A gesture was already reported this capture; ignore further input until
    /// capture is turned off again.
    done: bool,
    /// Bindable buttons currently down, with their press time (for tap vs hold).
    down: Vec<(Button, Instant)>,
    /// When capture armed, for the idle-timeout cancel.
    armed_at: Option<Instant>,
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
    pub fn new(cfg: InputConfig) -> Self {
        Self {
            left: (0.0, 0.0),
            dpad: (0.0, 0.0),
            right: (0.0, 0.0),
            l2_down: false,
            r2_down: false,
            bindings: Bindings::load(),
            hold: Duration::from_millis(cfg.hold_ms),
            scroll_mode: cfg.starts_in_scroll_mode(),
            held: vec![],
            capture: Capture::default(),
            cfg,
        }
    }

    /// Replace the tunables (dead zone, trigger threshold, hold gesture) — the
    /// settings overlay editing them live. The derived `hold` duration is
    /// recomputed; device state and pending gestures are untouched.
    pub fn set_config(&mut self, cfg: InputConfig) {
        self.hold = Duration::from_millis(cfg.hold_ms);
        self.cfg = cfg;
    }

    /// Swap in a freshly built gesture table — the settings overlay rebinding
    /// controls live. Device state and pending gestures are untouched (a held
    /// button keeps resolving against whatever was bound when it went down).
    pub fn set_bindings(&mut self, bindings: Bindings) {
        self.bindings = bindings;
    }

    /// Enter / leave binding-capture mode (the settings overlay listening for a
    /// gamepad gesture). Idempotent — only the on/off transition resets state.
    /// On entry, the bindable buttons currently held (the A that opened capture)
    /// are remembered so capture arms only once they release.
    pub fn set_capture(&mut self, on: bool) {
        if on == self.capture.on {
            return;
        }
        let down: Vec<(Button, Instant)> = if on {
            let now = Instant::now();
            self.held
                .iter()
                .filter(|h| bindings::button_name(h.button).is_some())
                .map(|h| (h.button, now))
                .collect()
        } else {
            vec![]
        };
        let armed = on && down.is_empty();
        self.capture = Capture {
            on,
            armed,
            done: false,
            armed_at: armed.then(Instant::now),
            down,
        };
        // Capture takes over the pad; drop any pending normal-mode gestures so a
        // button held across the transition can't resolve as both.
        self.held.clear();
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
        // Capture keeps the loop ticking to time holds and the idle timeout.
        self.capture.on
            || self.aim() != (0.0, 0.0)
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
        // Capture freezes the sticks/triggers (no cursor, no tab cycling).
        if self.capture.on {
            return;
        }
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
        if self.capture.on {
            self.on_capture_button(button, pressed, commands);
            return;
        }
        // The D-pad contributes to the aim vector on both edges (per axis, so a
        // held diagonal keeps both), and emits a discrete press edge for hint
        // mode's combo symbols (ignored elsewhere).
        let dpad_dir = match button {
            Button::DPadLeft => Some((-1, 0)),
            Button::DPadRight => Some((1, 0)),
            Button::DPadUp => Some((0, -1)),
            Button::DPadDown => Some((0, 1)),
            _ => None,
        };
        if let Some((dx, dy)) = dpad_dir {
            if dx != 0 {
                self.dpad.0 = if pressed { dx as f32 } else { 0.0 };
            } else {
                self.dpad.1 = if pressed { dy as f32 } else { 0.0 };
            }
            if pressed {
                commands.push(AppCommand::Input(InputCommand::DpadPress(dx, dy)));
            }
            return;
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
        if self.capture.on {
            self.capture_tick(commands);
            return;
        }
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
            stick: self.left,
            scroll: self.right.1,
            scroll_mode: self.scroll_mode,
        }));
    }

    /// A button edge while capturing a binding. Resolves the same gestures as
    /// play: a quick press+release is a tap, two buttons down is a chord, and a
    /// single button held past the threshold is a hold (fired in [`capture_tick`]).
    /// The first resolved gesture is reported and capture goes quiet until reset.
    fn on_capture_button(&mut self, button: Button, pressed: bool, commands: &mut Vec<AppCommand>) {
        // Only bindable buttons count; the D-pad and triggers aren't bindable.
        if bindings::button_name(button).is_none() || self.capture.done {
            return;
        }
        let now = Instant::now();
        if pressed {
            if !self.capture.down.iter().any(|(b, _)| *b == button) {
                self.capture.down.push((button, now));
            }
            // Two buttons down (post-arm) is a chord, reported on the second press.
            if self.capture.armed && self.capture.down.len() >= 2 {
                let (a, b) = (self.capture.down[0].0, self.capture.down[1].0);
                if let Some(gesture) = bindings::chord_gesture(a, b) {
                    self.capture.done = true;
                    push_capture(commands, gesture);
                }
            }
            return;
        }

        let pressed_at = self
            .capture
            .down
            .iter()
            .position(|(b, _)| *b == button)
            .map(|i| self.capture.down.remove(i).1);
        if !self.capture.armed {
            // Still waiting for the activating press to clear; arm once it does.
            if self.capture.down.is_empty() {
                self.capture.armed = true;
                self.capture.armed_at = Some(now);
            }
            return;
        }
        // A button pressed and released before the hold threshold is a tap.
        if let Some(at) = pressed_at {
            if now.duration_since(at) < self.hold {
                if let Some(name) = bindings::button_name(button) {
                    self.capture.done = true;
                    push_capture(commands, name.to_string());
                }
            }
        }
    }

    /// Per-frame capture timing: a single button held past the threshold is a
    /// hold; armed with nothing pressed for [`CAPTURE_TIMEOUT`] cancels (the
    /// handheld escape hatch). No analog state is emitted while capturing.
    fn capture_tick(&mut self, commands: &mut Vec<AppCommand>) {
        if self.capture.done || !self.capture.armed {
            return;
        }
        let now = Instant::now();
        if self.capture.down.len() == 1 {
            let (button, at) = self.capture.down[0];
            if now.duration_since(at) >= self.hold {
                if let Some(name) = bindings::button_name(button) {
                    self.capture.done = true;
                    push_capture(commands, format!("hold:{name}"));
                }
            }
        } else if self.capture.down.is_empty()
            && self
                .capture
                .armed_at
                .is_some_and(|t| now.duration_since(t) >= CAPTURE_TIMEOUT)
        {
            self.capture.done = true;
            commands.push(AppCommand::Settings(SettingsAction::CaptureCancel));
        }
    }
}

/// Report a captured gamepad gesture to the settings overlay.
fn push_capture(commands: &mut Vec<AppCommand>, gesture: String) {
    commands.push(AppCommand::Settings(SettingsAction::CaptureBinding {
        gesture,
        keyboard: false,
    }));
}
