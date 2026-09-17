//! Game Mode's translator: the pad and the keyboard drive the game, not the
//! chrome. What each source sends is the active [`InputMap`]; a source with a
//! target is withheld from the page's raw input (the `bool` returns here), so
//! one press is never seen twice. Select is reserved in every map — held
//! past the hold it opens the Game Mode menu.

use super::input_map::{Dir, InputMap, KeyTarget, Side, StickRole, Target};
use crate::app::{AppCommand, InputCommand};
use crate::browser::AppBrowser;
use crate::config::InputConfig;
use crate::event::sdl2_servo::key_event;
use inputbind::sdl::axis_value;
use inputbind::{Pad, Trigger};
use sdl2::controller::Axis;
use std::time::{Duration, Instant};

/// Stick-to-direction hysteresis: engage at the dead zone, release below this
/// fraction of it, so a stick resting at the edge cannot spam edges.
const STICK_RELEASE_RATIO: f32 = 0.8;

/// Left, then right — the order [`InputMap::stick`] and the state arrays use.
const STICKS: [Side; 2] = Side::ALL;

/// Arrow directions down for a digital (-1/0/1) x/y pair, in [`Dir::ALL`] order.
fn dirs(x: i32, y: i32) -> [bool; 4] {
    [y < 0, y > 0, x < 0, x > 0]
}

/// One stick axis as a digital direction, with hysteresis.
fn axis_digital(value: f32, prev: i32, deadzone: f32) -> i32 {
    let release = deadzone * STICK_RELEASE_RATIO;
    if value >= deadzone {
        1
    } else if value <= -deadzone {
        -1
    } else if prev == 1 && value >= release {
        1
    } else if prev == -1 && value <= -release {
        -1
    } else {
        0
    }
}

/// Which stick an axis belongs to, and whether it is the Y one.
fn stick_axis(axis: Axis) -> Option<(usize, bool)> {
    Some(match axis {
        Axis::LeftX => (0, false),
        Axis::LeftY => (0, true),
        Axis::RightX => (1, false),
        Axis::RightY => (1, true),
        _ => return None,
    })
}

/// The pad a trigger axis stands for.
fn trigger_pad(axis: Axis) -> Option<Pad> {
    Some(match axis {
        Axis::TriggerLeft => Pad::L2,
        Axis::TriggerRight => Pad::R2,
        _ => return None,
    })
}

pub struct GameInput {
    map: InputMap,
    /// Key targets the page holds, and how many sources hold each: a D-pad and
    /// a stick can name one key, and the first release must not end it.
    held: Vec<(KeyTarget, u32)>,
    /// What each pad took when it went down: a layer may have been held then
    /// and released since, and the release owes the target the press sent.
    pads: Vec<Option<Target>>,
    /// What each held key took, for the same reason.
    keys: Vec<(u32, Target)>,
    /// Activators held, most recent last — which is the one that wins.
    active: Vec<usize>,
    /// Each stick's digital state per axis, for [`axis_digital`].
    digital: [(i32, i32); 2],
    /// Which of each stick's four directions the page is holding, so a target is
    /// released exactly once (see [`Dir::ALL`] for the bit order).
    engaged: [u8; 2],
    /// Each stick's raw vector, read by the analog roles.
    vectors: [(f32, f32); 2],
    /// Whether the page holds the left mouse button.
    click: bool,
    triggers: [Trigger; 2],
    /// When Select went down; held past `hold` it opens the Game Mode menu.
    select_at: Option<Instant>,
    deadzone: f32,
    hold: Duration,
}

impl GameInput {
    pub fn new(map: InputMap, cfg: &InputConfig) -> Self {
        Self {
            map,
            held: Vec::new(),
            pads: vec![None; Pad::COUNT],
            keys: Vec::new(),
            active: Vec::new(),
            digital: [(0, 0); 2],
            engaged: [0; 2],
            vectors: [(0.0, 0.0); 2],
            click: false,
            triggers: [
                Trigger::new(Pad::L2, cfg.trigger_threshold),
                Trigger::new(Pad::R2, cfg.trigger_threshold),
            ],
            select_at: None,
            deadzone: cfg.deadzone,
            hold: Duration::from_millis(cfg.hold_ms),
        }
    }

    /// Retuned in place, like [`super::gamepad::Gamepad::set_config`].
    pub fn set_config(&mut self, cfg: &InputConfig) {
        for trigger in &mut self.triggers {
            trigger.set_threshold(cfg.trigger_threshold);
        }
        self.deadzone = cfg.deadzone;
        self.hold = Duration::from_millis(cfg.hold_ms);
    }

    /// Switch map without leaving Game Mode (the menu's InputMap row): what
    /// the page holds under the old one is released before the new one starts.
    pub fn set_map(&mut self, map: InputMap, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        self.release(browser, commands);
        self.map = map;
    }

    pub fn map_id(&self) -> &str {
        &self.map.id
    }

    /// The layer held right now, if any: the most recent activator wins, and a
    /// button it leaves alone still falls through to the base.
    fn layer(&self) -> Option<usize> {
        self.active.last().copied()
    }

    /// One pad edge, from a controller button or a key-wired pad (Miyoo).
    /// Returns whether the source is bound here, i.e. withheld from the page.
    pub fn on_pad(
        &mut self,
        pad: Pad,
        pressed: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        // Select is reserved in every map: the hold that opens the menu.
        if pad == Pad::Select {
            self.select_at = match pressed {
                true => self.select_at.or_else(|| Some(Instant::now())),
                false => None,
            };
            return true;
        }
        let slot = pad as usize;
        match (pressed, self.pads[slot].clone()) {
            // An autorepeat from a key-wired pad: the press already resolved.
            (true, Some(target)) => bound(&target),
            (true, None) => {
                let Some(target) = self.map.pad(self.layer(), pad).cloned() else {
                    return false;
                };
                self.pads[slot] = Some(target.clone());
                self.fire(&target, true, browser, commands)
            }
            (false, Some(target)) => {
                self.pads[slot] = None;
                self.fire(&target, false, browser, commands)
            }
            // A release of a press that was never seen; nothing to unwind.
            (false, None) => self.map.pad(self.layer(), pad).is_some_and(bound),
        }
    }

    /// One axis. Returns whether the source is bound here (withheld from the page).
    pub fn on_axis(
        &mut self,
        axis: Axis,
        value: i16,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        let value = axis_value(value);
        // A trigger is a pad with a threshold; the edges it crosses are presses.
        if let Some(pad) = trigger_pad(axis) {
            let index = usize::from(pad == Pad::R2);
            let (released, pressed) = self.triggers[index].axis(value);
            for (edge, down) in [(released, false), (pressed, true)] {
                if edge.is_some() {
                    self.on_pad(pad, down, browser, commands);
                }
            }
            return self.map.pad(self.layer(), pad).is_some_and(bound);
        }
        let Some((index, is_y)) = stick_axis(axis) else {
            return false;
        };
        // Read the role before touching the state: both borrow `self`.
        let role = self.map.stick(STICKS[index]);
        // A stick told to send nothing keeps its axis anyway, or `none` and
        // `passthrough` would be the same thing written twice.
        if *role == StickRole::Analog(Target::None) {
            return true;
        }
        let (analog, digital_role) = (role.is_analog(), role.is_digital());
        // The whole stick is one vector; `tick` reads it each frame.
        if analog {
            match is_y {
                true => self.vectors[index].1 = value,
                false => self.vectors[index].0 = value,
            }
            return true;
        }
        if !digital_role {
            return false;
        }
        let digital = axis_digital(value, self.axis_state(index, is_y), self.deadzone);
        match is_y {
            true => self.digital[index].1 = digital,
            false => self.digital[index].0 = digital,
        }
        self.refresh_stick(index, browser, commands);
        // Half an axis cannot be withheld, so a stick read as directions keeps
        // the whole axis from the page.
        true
    }

    /// One physical key edge while the mode is on, resolved after the pad keymap
    /// (a Miyoo's D-pad is a pad, not a key). Returns whether it was consumed.
    pub fn on_key(
        &mut self,
        code: u32,
        pressed: bool,
        repeat: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        let held = self.keys.iter().position(|(c, _)| *c == code);
        // The OS repeat of a key we took would be a press with no release; one
        // we did not take is the game's, repeat and all.
        if repeat {
            return held.is_some_and(|i| bound(&self.keys[i].1));
        }
        match (pressed, held) {
            (true, Some(i)) => bound(&self.keys[i].1),
            (true, None) => {
                let Some(target) = self.map.key(self.layer(), code).cloned() else {
                    return false;
                };
                self.keys.push((code, target.clone()));
                self.fire(&target, true, browser, commands)
            }
            (false, Some(i)) => {
                let (_, target) = self.keys.swap_remove(i);
                self.fire(&target, false, browser, commands)
            }
            (false, None) => self.map.key(self.layer(), code).is_some_and(bound),
        }
    }

    /// Apply one source's edge. Returns whether it was consumed, i.e. withheld
    /// from the page's raw input.
    fn fire(
        &mut self,
        target: &Target,
        pressed: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        match target {
            Target::Key(key) => self.hold_key(key, pressed, browser),
            Target::Click => self.set_click(pressed, commands),
            // Refused at load for every source that reaches here.
            Target::Cursor { .. } | Target::Scroll { .. } => {}
            // An activator sends nothing of its own, which is what makes a
            // layer free of buffering: there is never a press to retract.
            Target::Layer(index) => self.hold_layer(*index, pressed),
            Target::Passthrough => return false,
            Target::None => {}
        }
        true
    }

    /// Take or release an activator's hold on its layer.
    fn hold_layer(&mut self, index: usize, pressed: bool) {
        match pressed {
            true => self.active.push(index),
            false => {
                if let Some(at) = self.active.iter().rposition(|held| *held == index) {
                    self.active.remove(at);
                }
            }
        }
    }

    /// Take or release one source's hold on a key. The page sees an edge only
    /// when the last source lets go, so two sources naming one key never send a
    /// release the other still wants.
    fn hold_key(&mut self, key: &KeyTarget, pressed: bool, browser: &AppBrowser) {
        let at = self.held.iter().position(|(held, _)| held == key);
        match (pressed, at) {
            (true, Some(i)) => self.held[i].1 += 1,
            (true, None) => {
                self.held.push((key.clone(), 1));
                send(browser, key, true);
            }
            (false, Some(i)) => {
                self.held[i].1 -= 1;
                if self.held[i].1 == 0 {
                    let (key, _) = self.held.swap_remove(i);
                    send(browser, &key, false);
                }
            }
            (false, None) => {}
        }
    }

    fn axis_state(&self, index: usize, is_y: bool) -> i32 {
        match is_y {
            true => self.digital[index].1,
            false => self.digital[index].0,
        }
    }

    /// Re-derive one stick's four directions and send the edges that changed.
    /// Only a direction that actually flipped is cloned out of the map —
    /// axis samples arrive in floods, edges do not.
    fn refresh_stick(
        &mut self,
        index: usize,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        let (x, y) = self.digital[index];
        let want = dirs(x, y);
        let mut edges: [Option<Target>; 4] = [None, None, None, None];
        {
            let StickRole::Digital(targets) = self.map.stick(STICKS[index]) else {
                return;
            };
            for dir in Dir::ALL {
                let slot = dir as usize;
                if want[slot] != self.digital_engaged(index, dir) {
                    edges[slot] = targets[slot].clone();
                }
            }
        }
        for dir in Dir::ALL {
            let slot = dir as usize;
            if want[slot] == self.digital_engaged(index, dir) {
                continue;
            }
            self.set_engaged(index, dir, want[slot]);
            if let Some(target) = &edges[slot] {
                self.fire(target, want[slot], browser, commands);
            }
        }
    }

    /// Whether a stick direction is currently counted as down. Derived from the
    /// engaged mask rather than the axes, which have already moved on.
    fn digital_engaged(&self, index: usize, dir: Dir) -> bool {
        self.engaged[index] & (1 << dir as u8) != 0
    }

    fn set_engaged(&mut self, index: usize, dir: Dir, on: bool) {
        let bit = 1u8 << dir as u8;
        match on {
            true => self.engaged[index] |= bit,
            false => self.engaged[index] &= !bit,
        }
    }

    /// The click follows the cursor through the router's Confirm intent.
    fn set_click(&mut self, pressed: bool, commands: &mut Vec<AppCommand>) {
        if self.click != pressed {
            self.click = pressed;
            commands.push(AppCommand::Input(InputCommand::Confirm(pressed)));
        }
    }

    /// Per-frame: fire a due Select hold, and emit whatever the sticks are
    /// bound to (the router moves the cursor and scrolls the page from it).
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        if let Some(at) = self.select_at {
            if at.elapsed() >= self.hold {
                self.select_at = None;
                commands.push(AppCommand::GameMode);
            }
        }
        let (aim, scroll) = self.analog();
        commands.push(AppCommand::Input(InputCommand::Analog {
            aim,
            stick: (0.0, 0.0),
            scroll,
            scroll_mode: false,
        }));
    }

    /// The cursor vector and the scroll amount the sticks ask for this frame,
    /// summed so a map may put both on either stick.
    fn analog(&self) -> ((f32, f32), f32) {
        let mut aim = (0.0, 0.0);
        let mut scroll = 0.0;
        for (index, side) in STICKS.into_iter().enumerate() {
            let (speed, to_cursor) = match self.map.stick(side) {
                StickRole::Analog(Target::Cursor { speed }) => (*speed, true),
                StickRole::Analog(Target::Scroll { speed }) => (*speed, false),
                _ => continue,
            };
            let (x, y) = self.vectors[index];
            let live = |v: f32| match v.abs() < self.deadzone {
                true => 0.0,
                false => v * speed,
            };
            match to_cursor {
                true => aim = (aim.0 + live(x), aim.1 + live(y)),
                false => scroll += live(y),
            }
        }
        (
            (aim.0.clamp(-1.0, 1.0), aim.1.clamp(-1.0, 1.0)),
            scroll.clamp(-1.0, 1.0),
        )
    }

    /// Whether the loop must keep ticking: a stick still driving something, or
    /// a hold pending.
    pub fn is_active(&self) -> bool {
        let (aim, scroll) = self.analog();
        aim != (0.0, 0.0) || scroll != 0.0 || self.select_at.is_some()
    }

    /// Release everything the page holds — a mode or focus transition must
    /// never leave it with a stuck key or a stuck click.
    pub fn release(&mut self, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        for (key, _) in self.held.drain(..) {
            send(browser, &key, false);
        }
        self.pads.fill(None);
        self.keys.clear();
        self.active.clear();
        self.digital = [(0, 0); 2];
        self.engaged = [0; 2];
        self.vectors = [(0.0, 0.0); 2];
        self.set_click(false, commands);
        self.select_at = None;
    }
}

/// Whether a target withholds its source from the page's raw input.
fn bound(target: &Target) -> bool {
    !matches!(target, Target::Passthrough)
}

/// One synthesized key edge to the page.
fn send(browser: &AppBrowser, key: &KeyTarget, down: bool) {
    let event = key_event(key.key.clone(), key.code, key.modifiers, down);
    browser.handle_input(servo::InputEvent::Keyboard(event));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_hysteresis_engages_at_the_deadzone_and_releases_below_it() {
        let dz = 0.5;
        assert_eq!(axis_digital(0.4, 0, dz), 0);
        assert_eq!(axis_digital(0.5, 0, dz), 1);
        // Wobbling just under the engage point holds the direction...
        assert_eq!(axis_digital(0.45, 1, dz), 1);
        // ...until it falls below the release point.
        assert_eq!(axis_digital(0.39, 1, dz), 0);
        // The opposite sign never inherits the hold.
        assert_eq!(axis_digital(0.45, -1, dz), 0);
        assert_eq!(axis_digital(-0.6, 1, dz), -1);
    }

    #[test]
    fn a_diagonal_holds_both_directions() {
        assert_eq!(dirs(1, -1), [true, false, false, true]);
        assert_eq!(dirs(0, 0), [false; 4]);
    }
}
