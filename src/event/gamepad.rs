//! Translates raw controller input into [`AppCommand`]s. Gesture resolution
//! belongs to [`inputbind::PadState`]; this holds the analog state it does not
//! model — the aim vector (left stick + D-pad) and the right stick's scroll —
//! and turns resolved [`Action`]s into commands.
//!
//! The D-pad and L2/R2 keep their analog and contextual roles *and* feed the pad
//! machine, so they are bindable without losing what they already do.
//!
//! [`Action::Scroll`] is handled here rather than routed: it latches
//! `scroll_mode`, which turns the aim vector into page scrolling.

use crate::app::{AppCommand, InputCommand};
use crate::config::InputConfig;
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
    cfg: InputConfig,
    pads: PadState<Action>,
    left_trigger: Trigger,
    right_trigger: Trigger,
    /// While latched (the `scroll` action toggles it), the aim vector scrolls
    /// the page instead of moving the cursor.
    scroll_mode: bool,
    /// Scratch for resolved actions, reused so the input path never allocates.
    actions: Vec<(Action, Edge)>,
}

impl Gamepad {
    pub fn new(cfg: InputConfig) -> Self {
        Self {
            left: Stick::new(cfg.deadzone),
            right: Stick::new(cfg.deadzone),
            dpad: (0.0, 0.0),
            pads: PadState::new(hold_of(&cfg), cadence_of(&cfg)),
            left_trigger: Trigger::new(Pad::L2, cfg.trigger_threshold),
            right_trigger: Trigger::new(Pad::R2, cfg.trigger_threshold),
            scroll_mode: cfg.starts_in_scroll_mode(),
            actions: Vec::with_capacity(Pad::COUNT),
            cfg,
        }
    }

    /// Retuned in place, so a live edit cannot drop a gesture in flight.
    pub fn set_config(&mut self, cfg: InputConfig) {
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
            || self.right.vector().1 != 0.0
            || self.pads.next_deadline(Instant::now()).is_some()
    }

    /// The gesture machine pairs a held action's edges; this carries them through.
    fn dispatch(&mut self, commands: &mut Vec<AppCommand>) {
        let mut actions = std::mem::take(&mut self.actions);
        for &(action, edge) in &actions {
            let pressed = edge == Edge::Press;
            if action == Action::Scroll {
                // Not a held action, so only its press ever arrives.
                self.scroll_mode = !self.scroll_mode;
                continue;
            }
            commands.extend(action.command(pressed));
        }
        actions.clear();
        self.actions = actions;
    }

    pub fn on_axis(
        &mut self,
        axis: Axis,
        value: i16,
        bindings: &Bindings<Action>,
        commands: &mut Vec<AppCommand>,
    ) {
        // L2/R2 are throttle-style axes: the router reads the intent as the
        // on-screen keyboard's Shift/Enter, or tab cycling. They feed the pad
        // machine too, so `l2`/`r2` gestures are bindable like any other.
        if let Some(pad) = trigger_of(axis) {
            let edges = self.trigger_edges(axis, value);
            let right = pad == Pad::R2;
            for (edge, pressed) in [(edges.0, false), (edges.1, true)] {
                if edge.is_some() {
                    commands.push(AppCommand::Input(InputCommand::Trigger { right, pressed }));
                }
            }
            self.feed_edges(edges, bindings, commands);
            return;
        }

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

    /// Feed a (released, pressed) edge pair to the pad machine, release first so
    /// a change ends the old hold before starting the new one.
    fn feed_edges(
        &mut self,
        edges: (Option<Pad>, Option<Pad>),
        bindings: &Bindings<Action>,
        commands: &mut Vec<AppCommand>,
    ) {
        let (released, pressed) = edges;
        if let Some(pad) = released {
            self.press(pad, false, bindings, commands);
        }
        if let Some(pad) = pressed {
            self.press(pad, true, bindings, commands);
        }
    }

    pub fn on_button(
        &mut self,
        button: Button,
        pressed: bool,
        bindings: &Bindings<Action>,
        commands: &mut Vec<AppCommand>,
    ) {
        let pad = pad_of(button);
        // The D-pad contributes to the aim vector on both edges (per axis, so a
        // held diagonal keeps both), and emits a discrete press edge for hint
        // mode's combo symbols (ignored elsewhere). It is also a bindable pad.
        if let Some((dx, dy)) = pad.and_then(Pad::vector) {
            if dx != 0 {
                self.dpad.0 = if pressed { dx as f32 } else { 0.0 };
            } else {
                self.dpad.1 = if pressed { dy as f32 } else { 0.0 };
            }
            if pressed {
                commands.push(AppCommand::Input(InputCommand::DpadPress(dx, dy)));
            }
        }

        if let Some(pad) = pad {
            self.press(pad, pressed, bindings, commands);
        }
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
    /// or repeat that just came due. The aim vector is sent raw along with the
    /// latched scroll-mode flag — what it means is the router's decision.
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        self.pads.tick(Instant::now(), &mut self.actions);
        self.dispatch(commands);

        commands.push(AppCommand::Input(InputCommand::Analog {
            aim: self.aim(),
            stick: self.left.vector(),
            scroll: self.right.vector().1,
            scroll_mode: self.scroll_mode,
        }));
    }
}

fn hold_of(cfg: &InputConfig) -> Duration {
    Duration::from_millis(cfg.hold_ms)
}

fn cadence_of(cfg: &InputConfig) -> Cadence {
    Cadence {
        initial_delay: Duration::from_millis(cfg.osk_nav_initial_delay_ms),
        interval: Duration::from_millis(cfg.osk_nav_repeat_ms),
    }
}
