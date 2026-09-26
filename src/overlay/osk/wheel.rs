//! The wheel style: the left stick aims at one of [`WHEEL_SECTORS`] groups
//! and a face button types the group's character on its own side of the pad.
//! Centred, the face buttons keep their grid meanings, A being Enter. R2 holds
//! the digits layer up.

use super::{Key, Osk, OskCommand, OskTarget, PadInput, Reading};
use crate::browser::AppBrowser;
use crate::config::{Face, FacePlaces, PadLayout};
use std::f32::consts::TAU;

/// The wheel's groups: one per stick direction, clockwise from up.
pub const WHEEL_SECTORS: usize = 8;

/// The order a group lists its characters in, one per face.
pub const FACES: [Face; 4] = [Face::West, Face::North, Face::East, Face::South];

/// How far past a group's edge the stick must swing before the next group takes
/// over, in groups, so a stick resting on a boundary does not flicker.
const WHEEL_EDGE_HYSTERESIS: f32 = 0.15;

/// The share of the aim threshold the stick may sag to and keep its group, so
/// easing off while pressing a face button does not drop to the centre.
const WHEEL_RELEASE: f32 = 0.6;

/// The wheel's state; layout and modifiers stay the keyboard's.
pub(super) struct Wheel {
    /// The group the stick aims at; `None` while it is centred.
    sector: Option<usize>,
    /// Whether R2 holds the digits layer up.
    digits: bool,
    places: FacePlaces,
}

impl Wheel {
    pub(super) fn new(layout: PadLayout) -> Self {
        Self {
            sector: None,
            digits: false,
            places: layout.places(),
        }
    }

    pub(super) fn set_pad_layout(&mut self, layout: PadLayout) {
        self.places = layout.places();
    }

    /// Drop the aim.
    pub(super) fn centre(&mut self) {
        self.sector = None;
    }

    pub(super) fn set_digits(&mut self, held: bool) {
        self.digits = held;
    }

    /// Aim with a stick vector (SDL axes, y down); `threshold` is the deflection
    /// that picks a group. Returns whether the aimed group changed.
    fn aim(&mut self, (x, y): (f32, f32), threshold: f32) -> bool {
        let len = x.hypot(y);
        let hold = self.sector.is_some() && len >= threshold * WHEEL_RELEASE;
        let sector = if len < threshold && !hold {
            None
        } else {
            let n = WHEEL_SECTORS as f32;
            let pos = x.atan2(-y).rem_euclid(TAU) / TAU * n;
            match self.sector {
                Some(s) if circular_distance(pos, s as f32, n) <= 0.5 + WHEEL_EDGE_HYSTERESIS => {
                    Some(s)
                }
                _ => Some(pos.round() as usize % WHEEL_SECTORS),
            }
        };
        let changed = sector != self.sector;
        self.sector = sector;
        changed
    }

    /// What `input` means on the wheel.
    pub(super) fn read(&mut self, input: PadInput) -> Reading {
        match input {
            PadInput::Stick(stick, threshold) => Reading::Drawn(self.aim(stick, threshold)),
            _ => self.command(input).map_or(Reading::Pass, Reading::Command),
        }
    }

    /// What a button `input` means on the wheel; the stick is [`Self::read`]'s.
    pub(super) fn command(&self, input: PadInput) -> Option<OskCommand> {
        // Aimed, a face types its corner; centred, it keeps its grid meaning.
        let face = |face, centred| match self.sector {
            Some(_) => OskCommand::Face(face),
            None => centred,
        };
        let cmd = match input {
            PadInput::A => face(self.places.a, OskCommand::Enter),
            PadInput::B => face(self.places.b, OskCommand::Hide),
            PadInput::X => face(self.places.x, OskCommand::Backspace),
            PadInput::Y => face(self.places.y, OskCommand::Space),
            PadInput::Shoulder(delta) if delta < 0 => OskCommand::Backspace,
            PadInput::Shoulder(_) => OskCommand::Space,
            PadInput::LeftTrigger(held) => OskCommand::Shift(held),
            PadInput::RightTrigger(held) => OskCommand::Digits(held),
            // The stick owns the wheel, so the D-pad slides the caret.
            PadInput::Dpad(dx, _) if dx < 0 => OskCommand::Press(Key::Left),
            PadInput::Dpad(dx, _) if dx > 0 => OskCommand::Press(Key::Right),
            PadInput::Start => OskCommand::Press(Key::Tab),
            PadInput::Select => OskCommand::Press(Key::Lang),
            PadInput::Dpad(..)
            | PadInput::Nav(..)
            | PadInput::Stick(..)
            | PadInput::Move(..)
            | PadInput::R3 => return None,
        };
        Some(cmd)
    }
}

impl Osk {
    /// The aimed group, if any.
    pub fn sector(&self) -> Option<usize> {
        self.wheel.sector
    }

    /// The character `face` types in `sector` of the current layer, under the
    /// current Shift and Caps.
    pub fn wheel_char(&self, sector: usize, face: Face) -> char {
        let index = FACES
            .iter()
            .position(|f| *f == face)
            .expect("FACES lists every face");
        let slot = (sector, index);
        self.layout()
            .wheel_char(self.wheel.digits, slot, self.shift(), self.caps)
    }

    /// Whether R2 holds the digits layer up.
    pub fn digits(&self) -> bool {
        self.wheel.digits
    }

    /// Type `face`'s corner of the aimed group.
    pub(super) fn wheel_face(&mut self, face: Face, target: OskTarget, browser: &AppBrowser) {
        if let Some(sector) = self.wheel.sector {
            let shift = self.shift();
            let c = self.wheel_char(sector, face);
            self.input_char(target, c, shift, browser);
            self.shift_once = false;
        }
    }
}

/// The distance between two positions on a circle `n` long.
fn circular_distance(a: f32, b: f32, n: f32) -> f32 {
    let d = (a - b).rem_euclid(n);
    d.min(n - d)
}

#[cfg(test)]
mod tests {
    use super::super::layout::{DIGITS, DIGITS_SHIFTED, LAYOUTS, LEFT_OF_ONE};
    use super::*;
    use crate::config::{OskConfig, OskStyle};

    fn wheel() -> Osk {
        let mut osk = Osk::new(&OskConfig::default(), PadLayout::default());
        osk.set_style(OskStyle::Wheel);
        osk
    }

    /// A short group is a face button that panics, a long one a character
    /// nothing can type.
    #[test]
    fn every_wheel_group_has_one_character_per_face() {
        for def in LAYOUTS {
            for group in def.letters.iter().chain(&DIGITS).chain(&DIGITS_SHIFTED) {
                assert_eq!(group.chars().count(), FACES.len(), "{} {group}", def.name);
            }
        }
    }

    #[test]
    fn the_stick_picks_groups_clockwise_from_up() {
        let mut osk = wheel();
        let dirs = [
            (0.0, -1.0),
            (0.7, -0.7),
            (1.0, 0.0),
            (0.7, 0.7),
            (0.0, 1.0),
            (-0.7, 0.7),
            (-1.0, 0.0),
            (-0.7, -0.7),
        ];
        for (sector, dir) in dirs.into_iter().enumerate() {
            osk.read(PadInput::Stick(dir, 0.5));
            assert_eq!(osk.sector(), Some(sector), "{dir:?}");
        }
        osk.read(PadInput::Stick((0.0, 0.0), 0.5));
        assert_eq!(osk.sector(), None);
    }

    /// A stick resting on the up/up-right boundary must not flicker between them.
    #[test]
    fn a_group_holds_past_its_edge() {
        let mut osk = wheel();
        osk.read(PadInput::Stick((0.0, -1.0), 0.5));
        let just_past = (TAU / WHEEL_SECTORS as f32) * 0.55;
        osk.read(PadInput::Stick((just_past.sin(), -just_past.cos()), 0.5));
        assert_eq!(osk.sector(), Some(0));
    }

    /// Easing off while pressing must not turn a letter into a centred press.
    #[test]
    fn a_sagging_stick_keeps_its_group() {
        let mut osk = wheel();
        osk.read(PadInput::Stick((1.0, 0.0), 0.5));
        osk.read(PadInput::Stick((0.4, 0.0), 0.5));
        assert_eq!(osk.sector(), Some(2));
        let b = osk.wheel.places.b;
        assert_eq!(osk.read(PadInput::B), Reading::Command(OskCommand::Face(b)));
    }

    /// Centred, the faces keep their grid meaning, wherever the layout prints them.
    #[test]
    fn centred_the_faces_keep_their_grid_meaning() {
        for layout in [PadLayout::Nintendo, PadLayout::Xbox, PadLayout::PlayStation] {
            let mut osk = wheel();
            osk.set_pad_layout(layout);
            assert_eq!(osk.read(PadInput::A), Reading::Command(OskCommand::Enter));
            assert_eq!(osk.read(PadInput::B), Reading::Command(OskCommand::Hide));
            assert_eq!(
                osk.read(PadInput::X),
                Reading::Command(OskCommand::Backspace)
            );
            assert_eq!(osk.read(PadInput::Y), Reading::Command(OskCommand::Space));
        }
    }

    #[test]
    fn r2_holds_the_digits_layer() {
        let mut osk = wheel();
        for held in [true, false] {
            let digits = Reading::Command(OskCommand::Digits(held));
            assert_eq!(osk.read(PadInput::RightTrigger(held)), digits);
        }
        assert_eq!(osk.wheel_char(0, Face::West), 'a');
        osk.wheel.set_digits(true);
        assert_eq!(osk.wheel_char(0, Face::West), '1');
    }

    /// A key picker needs the named keys the wheel cannot reach.
    #[test]
    fn a_picker_gets_the_grid() {
        let mut osk = wheel();
        osk.set_picking(true);
        assert!(!osk.wheel());
        assert_eq!(osk.read(PadInput::Stick((1.0, 0.0), 0.5)), Reading::Pass);
    }

    /// Every character the wheel of the `lang`th default layout can type.
    fn typeable(lang: usize) -> std::collections::HashSet<char> {
        let mut osk = wheel();
        osk.lang = lang;
        let mut typed = std::collections::HashSet::new();
        for shift in [false, true] {
            osk.shift_once = shift;
            for digits in [false, true] {
                osk.wheel.digits = digits;
                for sector in 0..WHEEL_SECTORS {
                    typed.extend(FACES.map(|face| osk.wheel_char(sector, face)));
                }
            }
        }
        typed
    }

    /// The ASCII symbols and digits the `lang`th default layout's wheel lacks.
    fn missing_ascii(lang: usize) -> String {
        let typed = typeable(lang);
        ('!'..='~')
            .filter(|c| !c.is_ascii_alphabetic() && !typed.contains(c))
            .collect()
    }

    /// No symbol needs the grid.
    #[test]
    fn the_wheel_reaches_every_symbol() {
        assert_eq!(missing_ascii(0), "");
        assert_eq!(missing_ascii(1), "");
    }

    /// Russian puts ё in the left-of-1 slot, as its keyboard does.
    #[test]
    fn the_russian_wheel_types_yo() {
        let typed = typeable(1);
        assert!(typed.contains(&'ё') && typed.contains(&'Ё') && typed.contains(&'№'));
    }

    /// A shifted slot must add a symbol, never repeat one the layer shows. The
    /// left-of-1 slot is the layout's key, not the layer's.
    #[test]
    fn the_shifted_digits_layer_repeats_nothing() {
        let plain: Vec<char> = DIGITS
            .iter()
            .flat_map(|g| g.chars())
            .map(|c| if c == LEFT_OF_ONE { 'ё' } else { c })
            .collect();
        let shifted = DIGITS_SHIFTED.iter().flat_map(|g| g.chars());
        for (i, c) in shifted.enumerate() {
            let moved = c != plain[i];
            assert!(!moved || !plain.contains(&c), "{c} is already on the layer");
        }
    }

    #[test]
    fn shift_changes_letters_and_the_digits_layer() {
        let mut osk = wheel();
        osk.shift_once = true;
        assert_eq!(osk.wheel_char(0, Face::West), 'A');
        assert_eq!(osk.wheel_char(7, Face::East), '-');
        osk.wheel.set_digits(true);
        assert_eq!(osk.wheel_char(0, Face::West), '1');
        assert_eq!(osk.wheel_char(2, Face::East), '>');
        assert_eq!(osk.wheel_char(5, Face::North), '`');
        assert_eq!(osk.wheel_char(7, Face::West), '`');
    }
}
