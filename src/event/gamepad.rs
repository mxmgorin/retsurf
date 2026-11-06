//! Translates raw controller input into [`AppCommand`]s. Gesture resolution
//! belongs to [`inputbind::PadState`]; this holds the analog state it does not
//! model — the aim vector (left stick + D-pad) and the right stick's scroll —
//! and turns resolved [`Action`]s into commands.
//!
//! The D-pad and L2/R2 keep their analog and contextual roles *and* feed the pad
//! machine, so they are bindable without losing what they already do.

use crate::command::{Analog, AppCommand, InputCommand};
use crate::config::{ControlsConfig, StickRole};
use crate::event::bindings::Action;
use inputbind::sdl::{axis_value, pad_of, trigger_of};
use inputbind::{Bindings, Cadence, Edge, Pad, PadState, Stick, Trigger};
use sdl2::controller::{Axis, Button};
use std::time::{Duration, Instant};

pub struct Gamepad {
    /// Its directions are not fed to the pad machine: a push would double up
    /// with the D-pad already going in.
    left: Stick,
    right: Stick,
    /// Digital -1/0/1, merged with the left stick into the aim vector.
    dpad: (f32, f32),
    /// Tunables (dead zones, trigger threshold, hold) from the config file.
    pub cfg: ControlsConfig,
    pads: PadState<Action>,
    left_trigger: Trigger,
    right_trigger: Trigger,
    /// Scratch for resolved actions, reused so the input path never allocates.
    actions: Vec<(Action, Edge)>,
    /// The cursor and scroll actions held down, one entry per source holding one.
    held: Vec<Action>,
}

impl Gamepad {
    pub fn new(cfg: ControlsConfig) -> Self {
        Self {
            left: Stick::new(cfg.deadzone),
            right: Stick::new(cfg.deadzone),
            dpad: (0.0, 0.0),
            pads: PadState::new(hold_of(&cfg), cadence_of(&cfg)),
            left_trigger: Trigger::new(Pad::L2, cfg.trigger_threshold),
            right_trigger: Trigger::new(Pad::R2, cfg.trigger_threshold),
            actions: Vec::with_capacity(Pad::COUNT),
            held: Vec::new(),
            cfg,
        }
    }

    /// Retuned in place, so a live edit cannot drop a gesture in flight.
    pub fn set_config(&mut self, cfg: ControlsConfig) {
        self.pads.set_timing(hold_of(&cfg), cadence_of(&cfg));
        self.left.set_deadzone(cfg.deadzone);
        self.right.set_deadzone(cfg.deadzone);
        self.left_trigger.set_threshold(cfg.trigger_threshold);
        self.right_trigger.set_threshold(cfg.trigger_threshold);
        self.cfg = cfg;
    }

    /// Forget every pad down, for a bindings reload or when capture takes over.
    /// The D-pad goes too, since capture may have swallowed its release; the
    /// sticks stay, as an axis only reports on change.
    pub fn reset(&mut self, commands: &mut Vec<AppCommand>) {
        self.pads.reset(&mut self.actions);
        self.dispatch(commands);
        self.dpad = (0.0, 0.0);
    }

    /// Capture feeds triggers here too, so their engaged state lives in one place.
    pub fn trigger_edges(&mut self, axis: Axis, value: i16) -> (Option<Pad>, Option<Pad>) {
        let value = axis_value(value);
        match trigger_of(axis) {
            Some(Pad::L2) => self.left_trigger.axis(value),
            Some(Pad::R2) => self.right_trigger.axis(value),
            _ => (None, None),
        }
    }

    /// The pads down right now, so capture knows which press activated it.
    pub fn held(&self) -> Vec<Pad> {
        self.pads.held_pads()
    }

    /// Combined aim vector (left stick + D-pad), clamped to -1..=1.
    fn aim(&self) -> (f32, f32) {
        let (x, y) = self.left.vector();
        (
            (x + self.dpad.0).clamp(-1.0, 1.0),
            (y + self.dpad.1).clamp(-1.0, 1.0),
        )
    }

    /// Whether the loop should keep ticking at ~60fps: to animate cursor/scroll,
    /// and to time pending holds and repeats (no SDL event marks either).
    pub fn is_active(&self) -> bool {
        self.aim() != (0.0, 0.0)
            || self.right.vector() != (0.0, 0.0)
            || !self.held.is_empty()
            || self.pads.next_deadline(Instant::now()).is_some()
    }

    /// Hold or let go of a cursor or scroll action, for one source.
    pub fn hold(&mut self, action: Action, pressed: bool) {
        if pressed {
            self.held.push(action);
        } else if let Some(at) = self.held.iter().position(|held| *held == action) {
            self.held.swap_remove(at);
        }
    }

    /// The gesture machine pairs a held action's edges; this carries them through.
    fn dispatch(&mut self, commands: &mut Vec<AppCommand>) {
        let mut actions = std::mem::take(&mut self.actions);
        for &(action, edge) in &actions {
            let pressed = edge == Edge::Press;
            if action.motion().is_some() {
                self.hold(action, pressed);
                continue;
            }
            commands.extend(action.command(pressed));
        }
        actions.clear();
        self.actions = actions;
    }

    /// A stick axis; the triggers arrive as pad edges ([`Self::trigger_edges`]).
    pub fn on_axis(&mut self, axis: Axis, value: i16) {
        // The sticks drive the cursor and the scroll, so their direction edges
        // are dropped; the deadzone is the Stick's and applies to the vector.
        let value = axis_value(value);
        match axis {
            Axis::LeftX => _ = self.left.axis(true, value),
            Axis::LeftY => _ = self.left.axis(false, value),
            Axis::RightX => _ = self.right.axis(true, value),
            Axis::RightY => _ = self.right.axis(false, value),
            _ => {}
        }
    }

    /// One pad edge, however it reached us — a controller button, or a key on a
    /// device that wires its pad to the keyboard (see [`inputbind::sdl::Keymap`]).
    pub fn on_pad(
        &mut self,
        pad: Pad,
        pressed: bool,
        bindings: &Bindings<Action>,
        commands: &mut Vec<AppCommand>,
    ) {
        // The D-pad feeds the aim vector on both edges, per axis so a held
        // diagonal keeps both, and a press edge for hint mode's combo symbols.
        if let Some((dx, dy)) = pad.vector() {
            if dx != 0 {
                self.dpad.0 = if pressed { dx as f32 } else { 0.0 };
            } else {
                self.dpad.1 = if pressed { dy as f32 } else { 0.0 };
            }
            if pressed {
                commands.push(AppCommand::Input(InputCommand::DpadPress(dx, dy)));
            }
        }

        // Triggers wired to keys never reach `on_axis`, where this intent is
        // otherwise made — the Miyoo sends L2/R2 as Tab and Backspace.
        if matches!(pad, Pad::L2 | Pad::R2) {
            commands.push(AppCommand::Input(InputCommand::Trigger {
                right: pad == Pad::R2,
                pressed,
            }));
        }

        self.press(pad, pressed, bindings, commands);
    }

    /// One pad edge through the gesture machine.
    fn press(
        &mut self,
        pad: Pad,
        pressed: bool,
        bindings: &Bindings<Action>,
        commands: &mut Vec<AppCommand>,
    ) {
        let now = Instant::now();
        if pressed {
            self.pads
                .on_press(pad, now, bindings, None, &mut self.actions);
        } else {
            self.pads.on_release(pad, now, &mut self.actions);
        }
        self.dispatch(commands);
    }

    /// Emit this frame's analog state for the router to apply, and fire any hold
    /// or repeat that just came due. The aim vector goes raw, for overlays; the
    /// page's vectors follow the roles in the config.
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        self.pads.tick(Instant::now(), &mut self.actions);
        self.dispatch(commands);

        commands.push(AppCommand::Input(InputCommand::Analog(Analog {
            aim: self.aim(),
            stick: self.left.vector(),
            cursor: self.driving(StickRole::Cursor, true, (0.0, 0.0)),
            cursor_uncurved: self.dpad_as(StickRole::Cursor),
            scroll: self.driving(StickRole::Scroll, false, self.dpad_as(StickRole::Scroll)),
            right: self.right.vector(),
        })));
    }

    /// What drives `role` on a page: the sticks given it, the actions held for
    /// it and `extra`, summed and clamped to -1..=1. `across`: whether the
    /// sticks count sideways too.
    fn driving(&self, role: StickRole, across: bool, extra: (f32, f32)) -> (f32, f32) {
        let sticks = [
            (&self.left, self.cfg.left_stick),
            (&self.right, self.cfg.right_stick),
        ]
        .into_iter()
        .filter(|(_, given)| *given == role)
        .map(|(stick, _)| stick.vector())
        .map(|(x, y)| (if across { x } else { 0.0 }, y));
        let held = self
            .held
            .iter()
            .filter_map(|action| action.motion())
            .filter(|(held, _)| *held == role)
            .map(|(_, dir)| dir);
        let (x, y) = sticks
            .chain(held)
            .chain([extra])
            .fold((0.0, 0.0), |(x, y), (dx, dy)| (x + dx, y + dy));
        (x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0))
    }

    /// The D-pad's vector while `role` is what it does on a page, else zero.
    fn dpad_as(&self, role: StickRole) -> (f32, f32) {
        match self.cfg.dpad == role {
            true => self.dpad,
            false => (0.0, 0.0),
        }
    }
}

/// A stick vector through the cursor's response curve: rescaled to start at the
/// dead zone's edge, then raised to `exponent`. Full deflection stays full, a
/// diagonal included.
pub fn cursor_curve((x, y): (f32, f32), deadzone: f32, exponent: f32) -> (f32, f32) {
    let length = x.hypot(y);
    if length <= deadzone {
        return (0.0, 0.0);
    }
    let t = ((length - deadzone) / (1.0 - deadzone)).min(1.0);
    let scale = t.powf(exponent) / length;
    (x * scale, y * scale)
}

/// The pad `button` reads as; `swap` exchanges A/B and X/Y.
pub fn labelled_pad(button: Button, swap: bool) -> Option<Pad> {
    let pad = pad_of(button)?;
    Some(match (swap, pad) {
        (true, Pad::A) => Pad::B,
        (true, Pad::B) => Pad::A,
        (true, Pad::X) => Pad::Y,
        (true, Pad::Y) => Pad::X,
        (_, pad) => pad,
    })
}

fn hold_of(cfg: &ControlsConfig) -> Duration {
    Duration::from_millis(cfg.hold_ms)
}

fn cadence_of(cfg: &ControlsConfig) -> Cadence {
    Cadence {
        initial_delay: Duration::from_millis(cfg.osk_nav_initial_delay_ms),
        interval: Duration::from_millis(cfg.osk_nav_repeat_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The curve starts at the dead zone's edge, keeps full deflection full,
    /// and slows the middle of the travel by its exponent.
    #[test]
    fn the_cursor_curve_slows_small_pushes_and_keeps_full_speed() {
        let dz = 0.25;
        assert_eq!(cursor_curve((0.25, 0.0), dz, 2.0), (0.0, 0.0));
        assert_eq!(cursor_curve((1.0, 0.0), dz, 2.0), (1.0, 0.0));
        let (half, _) = cursor_curve((0.625, 0.0), dz, 2.0);
        assert!(
            (half - 0.25).abs() < 1e-6,
            "half the travel at a quarter speed"
        );
        let (linear, _) = cursor_curve((0.625, 0.0), dz, 1.0);
        assert!((linear - 0.5).abs() < 1e-6);
        // A full diagonal is no faster than a full push along one axis.
        let (x, y) = cursor_curve((1.0, 1.0), dz, 2.0);
        assert!((x.hypot(y) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn swap_reads_the_face_buttons_across() {
        let read = |button| labelled_pad(button, true);
        assert_eq!(read(Button::B), Some(Pad::A));
        assert_eq!(read(Button::A), Some(Pad::B));
        assert_eq!(read(Button::Y), Some(Pad::X));
        assert_eq!(read(Button::X), Some(Pad::Y));
        assert_eq!(read(Button::Start), Some(Pad::Start));
    }

    #[test]
    fn unswapped_stays_positional() {
        assert_eq!(labelled_pad(Button::A, false), Some(Pad::A));
        assert_eq!(labelled_pad(Button::Y, false), Some(Pad::Y));
    }

    fn analog(pad: &mut Gamepad) -> Analog {
        let mut out = Vec::new();
        pad.tick(&mut out);
        out.into_iter()
            .find_map(|command| match command {
                AppCommand::Input(InputCommand::Analog(analog)) => Some(analog),
                _ => None,
            })
            .expect("every tick sends the analog state")
    }

    /// The page reads the sticks by their roles; overlays keep the physical
    /// left stick, and a stick set to none drives neither.
    #[test]
    fn the_sticks_drive_what_their_roles_say() {
        let cfg = ControlsConfig {
            left_stick: StickRole::Scroll,
            right_stick: StickRole::Cursor,
            ..ControlsConfig::default()
        };
        let mut pad = Gamepad::new(cfg.clone());
        pad.on_axis(Axis::LeftY, i16::MAX);
        pad.on_axis(Axis::RightX, i16::MAX);
        let swapped = analog(&mut pad);
        assert_eq!(swapped.cursor, (1.0, 0.0));
        assert_eq!(swapped.scroll, (0.0, 1.0));
        assert_eq!(swapped.stick, (0.0, 1.0));
        pad.set_config(ControlsConfig {
            left_stick: StickRole::None,
            ..cfg
        });
        let none = analog(&mut pad);
        assert_eq!(none.scroll, (0.0, 0.0));
        assert_eq!(none.cursor, (1.0, 0.0));
        pad.set_config(ControlsConfig {
            left_stick: StickRole::Cursor,
            right_stick: StickRole::Cursor,
            ..ControlsConfig::default()
        });
        assert_eq!(analog(&mut pad).cursor, (1.0, 1.0));
    }

    /// The page takes the D-pad by its role: uncurved as the cursor, both ways
    /// as scroll; overlays keep it whatever the role.
    #[test]
    fn the_dpad_drives_what_its_role_says() {
        let store = crate::event::bindings::default_store();
        let bindings = crate::event::bindings::build(&store, crate::event::test_key_names());
        for (role, uncurved, scroll) in [
            (StickRole::Cursor, (1.0, 0.0), (0.0, 0.0)),
            (StickRole::Scroll, (0.0, 0.0), (1.0, 0.0)),
            (StickRole::None, (0.0, 0.0), (0.0, 0.0)),
        ] {
            let mut pad = Gamepad::new(ControlsConfig {
                dpad: role,
                ..ControlsConfig::default()
            });
            pad.on_pad(Pad::Right, true, &bindings, &mut Vec::new());
            let page = analog(&mut pad);
            assert_eq!(
                (page.cursor_uncurved, page.scroll, page.aim),
                (uncurved, scroll, (1.0, 0.0)),
                "{role:?}"
            );
        }
    }

    /// Held actions push at full deflection, each source on its own: one let go
    /// keeps the other, and held scrolling goes sideways where a stick does not.
    #[test]
    fn held_actions_drive_the_cursor_and_scroll() {
        let mut pad = Gamepad::new(ControlsConfig::default());
        pad.hold(Action::CursorLeft, true);
        pad.hold(Action::CursorLeft, true);
        pad.hold(Action::ScrollRight, true);
        assert!(pad.is_active());
        let held = analog(&mut pad);
        assert_eq!(held.cursor, (-1.0, 0.0));
        assert_eq!(held.scroll, (1.0, 0.0));
        pad.hold(Action::CursorLeft, false);
        assert_eq!(analog(&mut pad).cursor, (-1.0, 0.0));
        pad.hold(Action::CursorLeft, false);
        pad.hold(Action::ScrollRight, false);
        let released = analog(&mut pad);
        assert_eq!((released.cursor, released.scroll), ((0.0, 0.0), (0.0, 0.0)));
        assert!(!pad.is_active());
        pad.on_axis(Axis::RightX, i16::MAX);
        assert_eq!(analog(&mut pad).scroll, (0.0, 0.0));
    }
}
