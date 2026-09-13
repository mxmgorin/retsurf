//! Game Mode's pad translator: the pad drives the game, not the chrome. The two
//! built-in mappings are [`crate::config::GameProfile`]; a bound source is
//! withheld from the Gamepad API (the `bool` returns here), so the page never
//! sees one press twice. Select is reserved in every profile — held past the
//! hold it leaves Game Mode.

use crate::app::{AppCommand, InputCommand};
use crate::browser::AppBrowser;
use crate::config::{GameProfile, InputConfig};
use crate::event::sdl2_servo::{char_keyboard_event, named_keyboard_event};
use inputbind::sdl::axis_value;
use inputbind::{Pad, Trigger};
use keyboard_types::{Code, NamedKey};
use sdl2::controller::Axis;
use std::time::{Duration, Instant};

/// Stick-to-arrow hysteresis: engage at the dead zone, release below this
/// fraction of it, so a stick resting at the edge cannot spam edges.
const STICK_RELEASE_RATIO: f32 = 0.8;

/// What a bound pad sends to the page — the same calls the OSK types with.
#[derive(Clone, Copy, PartialEq, Debug)]
enum KeyTarget {
    Char(char),
    Named(NamedKey, Code),
}

/// The arrows in [`dirs`] index order: up, down, left, right.
const ARROWS: [KeyTarget; 4] = [
    KeyTarget::Named(NamedKey::ArrowUp, Code::ArrowUp),
    KeyTarget::Named(NamedKey::ArrowDown, Code::ArrowDown),
    KeyTarget::Named(NamedKey::ArrowLeft, Code::ArrowLeft),
    KeyTarget::Named(NamedKey::ArrowRight, Code::ArrowRight),
];

/// The `keys` mapping for the non-directional pads — the retro convention
/// (z/x + Space/Enter) that PICO-8 exports and js13k entries share.
fn keys_target(pad: Pad) -> Option<KeyTarget> {
    Some(match pad {
        Pad::A => KeyTarget::Char(' '),
        Pad::B => KeyTarget::Char('z'),
        Pad::X => KeyTarget::Char('x'),
        Pad::Y => KeyTarget::Char('c'),
        Pad::L1 => KeyTarget::Char('q'),
        Pad::R1 => KeyTarget::Char('e'),
        Pad::Start => KeyTarget::Named(NamedKey::Enter, Code::Enter),
        _ => return None,
    })
}

/// Arrow directions down for a digital (-1/0/1) x/y pair.
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

/// One synthesized key edge to the page.
fn send(browser: &AppBrowser, target: KeyTarget, down: bool) {
    let event = match target {
        KeyTarget::Char(c) => char_keyboard_event(c, false, down),
        KeyTarget::Named(key, code) => named_keyboard_event(key, code, down),
    };
    browser.handle_input(servo::InputEvent::Keyboard(event));
}

pub struct GameInput {
    profile: GameProfile,
    /// D-pad digital state, per axis so a held diagonal keeps both.
    dpad: (i32, i32),
    /// Left stick as digital directions (see [`axis_digital`]).
    stick: (i32, i32),
    /// The arrow keys the page currently holds, d-pad and stick merged.
    arrows: [bool; 4],
    /// Non-directional pads currently down, for release on a transition.
    held: Vec<(Pad, KeyTarget)>,
    /// Right stick raw; the dead zone applies when it is read as the aim.
    right: (f32, f32),
    /// Whether the page holds the left mouse button (R2).
    click: bool,
    r2: Trigger,
    /// When Select went down; held past `hold` it leaves Game Mode.
    select_at: Option<Instant>,
    deadzone: f32,
    hold: Duration,
}

impl GameInput {
    pub fn new(profile: GameProfile, cfg: &InputConfig) -> Self {
        Self {
            profile,
            dpad: (0, 0),
            stick: (0, 0),
            arrows: [false; 4],
            held: Vec::new(),
            right: (0.0, 0.0),
            click: false,
            r2: Trigger::new(Pad::R2, cfg.trigger_threshold),
            select_at: None,
            deadzone: cfg.deadzone,
            hold: Duration::from_millis(cfg.hold_ms),
        }
    }

    /// Retuned in place, like [`super::gamepad::Gamepad::set_config`].
    pub fn set_config(&mut self, cfg: &InputConfig) {
        self.r2.set_threshold(cfg.trigger_threshold);
        self.deadzone = cfg.deadzone;
        self.hold = Duration::from_millis(cfg.hold_ms);
    }

    /// One pad edge, from a controller button or a key-wired pad (Miyoo).
    /// Returns whether the source is bound here, i.e. withheld from the API.
    pub fn on_pad(
        &mut self,
        pad: Pad,
        pressed: bool,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        // Select is reserved in every profile: the hold that leaves.
        if pad == Pad::Select {
            self.select_at = match pressed {
                true => self.select_at.or_else(|| Some(Instant::now())),
                false => None,
            };
            return true;
        }
        // R2 clicks in both profiles. The axis form lands in `on_axis`; this is
        // the key form (the Miyoo wires R2 to a key).
        if pad == Pad::R2 {
            self.set_click(pressed, commands);
            return true;
        }
        if self.profile == GameProfile::Pad {
            return false;
        }
        if let Some((dx, dy)) = pad.vector() {
            if dx != 0 {
                self.dpad.0 = if pressed { dx } else { 0 };
            } else {
                self.dpad.1 = if pressed { dy } else { 0 };
            }
            self.refresh_arrows(browser);
            return true;
        }
        let Some(target) = keys_target(pad) else {
            return false;
        };
        let down = self.held.iter().position(|(p, _)| *p == pad);
        match (pressed, down) {
            (true, None) => {
                self.held.push((pad, target));
                send(browser, target, true);
            }
            (false, Some(i)) => {
                self.held.swap_remove(i);
                send(browser, target, false);
            }
            // Autorepeat on a key-wired pad, or a release that never went down.
            _ => {}
        }
        true
    }

    /// One axis. Returns whether the source is bound here (withheld from the API).
    pub fn on_axis(
        &mut self,
        axis: Axis,
        value: i16,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> bool {
        let value = axis_value(value);
        // R2 clicks in both profiles; L2 stays the page's.
        if axis == Axis::TriggerRight {
            let (released, pressed) = self.r2.axis(value);
            if released.is_some() {
                self.set_click(false, commands);
            }
            if pressed.is_some() {
                self.set_click(true, commands);
            }
            return true;
        }
        // The right stick is the cursor in both profiles.
        match axis {
            Axis::RightX => {
                self.right.0 = value;
                return true;
            }
            Axis::RightY => {
                self.right.1 = value;
                return true;
            }
            _ => {}
        }
        if self.profile == GameProfile::Pad {
            return false;
        }
        match axis {
            Axis::LeftX => self.stick.0 = axis_digital(value, self.stick.0, self.deadzone),
            Axis::LeftY => self.stick.1 = axis_digital(value, self.stick.1, self.deadzone),
            // TriggerLeft: unbound, the page's.
            _ => return false,
        }
        self.refresh_arrows(browser);
        true
    }

    /// Re-derive the merged arrow state and send the page the edges that changed.
    fn refresh_arrows(&mut self, browser: &AppBrowser) {
        let d = dirs(self.dpad.0, self.dpad.1);
        let s = dirs(self.stick.0, self.stick.1);
        for (i, &target) in ARROWS.iter().enumerate() {
            let want = d[i] || s[i];
            if want != self.arrows[i] {
                self.arrows[i] = want;
                send(browser, target, want);
            }
        }
    }

    /// The click follows the cursor through the router's Confirm intent.
    fn set_click(&mut self, pressed: bool, commands: &mut Vec<AppCommand>) {
        if self.click != pressed {
            self.click = pressed;
            commands.push(AppCommand::Input(InputCommand::Confirm(pressed)));
        }
    }

    /// Per-frame: fire a due Select hold, and emit the right stick as the aim
    /// (the router moves the cursor and hovers the page from it).
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        if let Some(at) = self.select_at {
            if at.elapsed() >= self.hold {
                self.select_at = None;
                commands.push(AppCommand::ToggleGameMode);
            }
        }
        commands.push(AppCommand::Input(InputCommand::Analog {
            aim: self.aim(),
            stick: (0.0, 0.0),
            scroll: 0.0,
            scroll_mode: false,
        }));
    }

    /// Right stick with the dead zone applied, like [`inputbind::Stick::vector`].
    fn aim(&self) -> (f32, f32) {
        let live = |v: f32| if v.abs() < self.deadzone { 0.0 } else { v };
        (live(self.right.0), live(self.right.1))
    }

    /// Whether the loop must keep ticking: cursor gliding, or a hold pending.
    pub fn is_active(&self) -> bool {
        self.aim() != (0.0, 0.0) || self.select_at.is_some()
    }

    /// Release everything the page holds — a mode or focus transition must
    /// never leave it with a stuck key or a stuck click.
    pub fn release(&mut self, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        for (_, target) in self.held.drain(..) {
            send(browser, target, false);
        }
        for (down, &target) in self.arrows.iter_mut().zip(ARROWS.iter()) {
            if *down {
                *down = false;
                send(browser, target, false);
            }
        }
        self.dpad = (0, 0);
        self.stick = (0, 0);
        self.set_click(false, commands);
        self.select_at = None;
    }
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
    fn the_keys_table_covers_faces_and_shoulders_only() {
        assert_eq!(keys_target(Pad::A), Some(KeyTarget::Char(' ')));
        assert_eq!(
            keys_target(Pad::Start),
            Some(KeyTarget::Named(NamedKey::Enter, Code::Enter))
        );
        // Select and R2 are system (exit, click); L2 and the D-pad live elsewhere.
        assert_eq!(keys_target(Pad::Select), None);
        assert_eq!(keys_target(Pad::R2), None);
        assert_eq!(keys_target(Pad::L2), None);
        assert_eq!(keys_target(Pad::Up), None);
    }

    #[test]
    fn a_diagonal_holds_both_arrows() {
        assert_eq!(dirs(1, -1), [true, false, false, true]);
        assert_eq!(dirs(0, 0), [false; 4]);
    }
}
