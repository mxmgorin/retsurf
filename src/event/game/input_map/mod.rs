//! Game Mode's input maps: what each pad, stick direction and key sends to the
//! page while the mode is on. The built-ins live here in code and are always
//! offered; an `input_maps/<id>.toml` in the data dir under a built-in's id
//! replaces it, and deleting that file is what "reset to default" means.
//!
//! ```toml
//! name = "My game"
//!
//! [pad]                  # buttons and the D-pad, by inputbind's names
//! a = "key.Space"
//! b = "key.z"
//! r2 = "mouse.left"
//! l2 = "passthrough"     # reaches the page as the gamepad button it is
//!
//! [stick.left]           # four directions, through the dead zone
//! up = "key.ArrowUp"
//! [stick.right]
//! analog = "mouse.cursor"   # or mouse.scroll — the whole stick, not a direction
//!
//! [key]                  # physical keys, resolved after the pad keymap
//! w = "key.ArrowUp"
//!
//! [layer.aim.pad]        # while `l2 = "layer:aim"` is held
//! a = "key.Shift"
//! ```

mod raw;
mod store;

pub use raw::RawTarget;
pub use store::{built_in, load_all, new_id, pick};

use inputbind::sdl::KeyNames;
use inputbind::Pad;
use keyboard_types::{Code, Key, Modifiers};
use raw::RawInputMap;

/// The key a stick's whole-vector target is written under.
const ANALOG: &str = "analog";

/// How a key target is spelled. Every target names its device — `key.Space`,
/// `mouse.left` — the way the editor's rows name their source.
pub const KEY_PREFIX: &str = "key.";

/// How a gamepad-button target is spelled, under the same table name a pad
/// source is written in.
pub const PAD_PREFIX: &str = "pad.";

/// The table a stick is written in.
pub const STICK_PREFIX: &str = "stick.";

/// Which stick, as the file spells it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const ALL: [Side; 2] = [Side::Left, Side::Right];

    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    pub fn parse(name: &str) -> Option<Side> {
        Side::ALL.into_iter().find(|side| side.name() == name)
    }
}

/// The four stick directions, in the order [`Dir::ALL`] and the runtime's arrays
/// use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];

    pub fn name(self) -> &'static str {
        match self {
            Dir::Up => "up",
            Dir::Down => "down",
            Dir::Left => "left",
            Dir::Right => "right",
        }
    }

    /// The unit vector a source held this way steps by, in screen axes: y grows
    /// downward, which is also the sense the page scrolls in.
    pub fn step(self) -> (f32, f32) {
        match self {
            Dir::Up => (0.0, -1.0),
            Dir::Down => (0.0, 1.0),
            Dir::Left => (-1.0, 0.0),
            Dir::Right => (1.0, 0.0),
        }
    }

    /// The arrow key this direction stands for.
    pub fn arrow(self) -> &'static str {
        match self {
            Dir::Up => "ArrowUp",
            Dir::Down => "ArrowDown",
            Dir::Left => "ArrowLeft",
            Dir::Right => "ArrowRight",
        }
    }

    fn parse(name: &str) -> Option<Dir> {
        Dir::ALL.into_iter().find(|dir| dir.name() == name)
    }
}

/// One key edge the page receives. `code` is what most games branch on, so it
/// is derived where the standard spells it the same and said out loud where not.
#[derive(Clone, PartialEq, Debug)]
pub struct KeyTarget {
    pub key: Key,
    pub code: Code,
    pub modifiers: Modifiers,
}

/// Which mouse button a click target presses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickButton {
    Left,
    Right,
    Middle,
}

/// What a source does while the mode is on.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    Key(KeyTarget),
    /// A mouse button at the cursor, spelled `mouse.left` and friends. Left is
    /// the path measured on hardware, and the one the chrome also answers to.
    Click(ClickButton),
    /// A held source scrolling the page every frame, spelled
    /// `mouse.scroll.<direction>`. The vector is a unit step in that direction,
    /// scaled like a stick's deflection.
    ScrollBy {
        x: f32,
        y: f32,
        speed: f32,
    },
    /// A held source moving the cursor every frame, spelled
    /// `mouse.cursor.<direction>` — a D-pad pointing where a stick would.
    CursorBy {
        x: f32,
        y: f32,
        speed: f32,
    },
    /// A gamepad button in the page's Gamepad API, spelled `pad.<button>` — a
    /// keyboard driving a pad-only game, or a pad whose buttons are dealt out
    /// differently. Sent on the pad that produced the source, or on one the
    /// browser synthesizes where nothing did.
    Pad(Pad),
    /// The whole stick moves the cursor (analog sources only).
    Cursor {
        speed: f32,
    },
    /// The whole stick scrolls the page (analog sources only).
    Scroll {
        speed: f32,
    },
    /// Reaches the page as the raw event it is — the Gamepad API for a pad, the
    /// key itself for a key. The only target the page sees twice, by design.
    Passthrough,
    /// Consumed and dropped: the source is inert while the map is active.
    None,
    /// Holds a layer open while the source is held, and sends nothing itself —
    /// which is why an activator needs no buffering and can never leak.
    Layer(usize),
}

impl Target {
    /// Whether this drives a stick as a whole rather than an edge.
    fn is_analog(&self) -> bool {
        matches!(self, Target::Cursor { .. } | Target::Scroll { .. })
    }
}

impl StickRole {
    /// Whether the whole stick is read as a vector each frame.
    pub fn is_analog(&self) -> bool {
        matches!(self, StickRole::Analog(target) if target.is_analog())
    }

    pub fn is_digital(&self) -> bool {
        matches!(self, StickRole::Digital(_))
    }
}

/// A stick's role: four digital directions, or the whole stick as one vector.
#[derive(Clone, PartialEq, Debug)]
pub enum StickRole {
    /// Per-direction targets, fired through the dead zone with hysteresis.
    Digital(Box<[Option<Target>; 4]>),
    Analog(Target),
    Unbound,
}

/// One alternate set, held open by its activator. Only buttons and keys: a
/// stick that changed role mid-hold would have to release and re-engage its
/// directions, which buys less than it costs.
#[derive(Clone, Default)]
struct Layer {
    pad: Vec<Option<Target>>,
    keys: Vec<(u32, Target)>,
}

#[derive(Clone)]
pub struct InputMap {
    /// The file stem, or the built-in's id; `[game_mode] input_map` names this.
    pub id: String,
    /// What the menu shows.
    pub name: String,
    /// Whether the binary carries this id, so deleting its file restores the
    /// original rather than removing the map.
    pub builtin: bool,
    /// Whether `input_maps/<id>.toml` is there — what deleting removes, and the
    /// only thing a built-in has to delete.
    pub file: bool,
    pad: Vec<Option<Target>>,
    /// Left, then right.
    sticks: [StickRole; 2],
    /// Physical keys by SDL keycode, sorted for lookup.
    keys: Vec<(u32, Target)>,
    /// Alternate sets, in the order `[layer.<name>]` declares them.
    layers: Vec<Layer>,
    /// The file as written, kept so an edit can be saved without rebuilding
    /// what the editor does not touch (sticks, keys, layers, comments aside).
    raw: RawInputMap,
}

impl InputMap {
    /// What a pad sends, under the held layer if one names it — a button the
    /// layer leaves alone falls through to the base rather than going inert.
    pub fn pad(&self, layer: Option<usize>, pad: Pad) -> Option<&Target> {
        let from_layer = layer
            .and_then(|i| self.layers.get(i))
            .and_then(|l| l.pad.get(pad as usize))
            .and_then(Option::as_ref);
        from_layer.or_else(|| self.pad.get(pad as usize).and_then(Option::as_ref))
    }

    pub fn stick(&self, side: Side) -> &StickRole {
        &self.sticks[side as usize]
    }

    /// The same fall-through for a physical key.
    pub fn key(&self, layer: Option<usize>, code: u32) -> Option<&Target> {
        let from_layer = layer
            .and_then(|i| self.layers.get(i))
            .and_then(|l| find_key(&l.keys, code));
        from_layer.or_else(|| find_key(&self.keys, code))
    }
}

fn find_key(keys: &[(u32, Target)], code: u32) -> Option<&Target> {
    keys.binary_search_by_key(&code, |(c, _)| *c)
        .ok()
        .map(|i| &keys[i].1)
}

impl InputMap {
    /// What the file says this pad sends, for the editor's row.
    pub fn raw_pad(&self, pad: Pad) -> Option<&RawTarget> {
        self.raw.pad.get(pad.name())
    }

    /// Rewrite one pad's entry; `None` unbinds it (the page gets it raw again).
    pub fn set_raw_pad(&mut self, pad: Pad, target: Option<RawTarget>) {
        match target {
            Some(target) => self.raw.pad.insert(pad.name().to_string(), target),
            None => self.raw.pad.remove(pad.name()),
        };
    }

    /// The keys the file binds, by the name it spells them with — the editor
    /// has a row per entry, since which keys exist is the keyboard's business
    /// rather than a set the screen could list.
    pub fn raw_keys(&self) -> impl Iterator<Item = (&str, &RawTarget)> {
        self.raw
            .key
            .iter()
            .map(|(name, target)| (name.as_str(), target))
    }

    /// Rewrite one key's entry; `None` takes the line out of the file, which
    /// leaves the key reaching the page as itself.
    pub fn set_raw_key(&mut self, name: &str, target: Option<RawTarget>) {
        match target {
            Some(target) => self.raw.key.insert(name.to_string(), target),
            None => self.raw.key.remove(name),
        };
    }

    /// What the file says the whole stick does.
    pub fn raw_stick(&self, side: Side) -> Option<&RawTarget> {
        self.raw.stick.get(side.name())?.get(ANALOG)
    }

    /// What it says one of its directions does.
    pub fn raw_stick_dir(&self, side: Side, dir: Dir) -> Option<&RawTarget> {
        self.raw.stick.get(side.name())?.get(dir.name())
    }

    /// Whether the *file* reads this stick as four directions. The resolved
    /// role is a save behind while the editor is open, so the rows ask here.
    pub fn raw_stick_is_digital(&self, side: Side) -> bool {
        Dir::ALL
            .into_iter()
            .any(|dir| self.raw_stick_dir(side, dir).is_some())
    }

    /// Give the whole stick one target, dropping whatever directions it had:
    /// the file is one or the other, and one holding both resolves as the
    /// directions.
    pub fn set_raw_stick(&mut self, side: Side, target: Option<RawTarget>) {
        self.raw.stick.remove(side.name());
        if let Some(target) = target {
            let table = self.raw.stick.entry(side.name().to_string()).or_default();
            table.insert(ANALOG.to_string(), target);
        }
    }

    /// Set one direction, dropping the whole-stick entry for the same reason.
    pub fn set_raw_stick_dir(&mut self, side: Side, dir: Dir, target: Option<RawTarget>) {
        let table = self.raw.stick.entry(side.name().to_string()).or_default();
        table.remove(ANALOG);
        match target {
            Some(target) => table.insert(dir.name().to_string(), target),
            None => table.remove(dir.name()),
        };
        // An empty table would write a `[stick.left]` header with nothing under it.
        if table.is_empty() {
            self.raw.stick.remove(side.name());
        }
    }

    /// Put the arrows on all four directions: a stick with none set resolves to
    /// unbound, which reaches the page instead.
    pub fn set_raw_stick_arrows(&mut self, side: Side) {
        for dir in Dir::ALL {
            let arrow = RawTarget::Short(format!("{KEY_PREFIX}{}", dir.arrow()));
            self.set_raw_stick_dir(side, dir, Some(arrow));
        }
    }

    /// Rename what the menu shows. The id stays: it is the file's stem, and
    /// `[game_mode] input_map` names it.
    pub fn set_name(&mut self, name: String) {
        self.raw.name = Some(name.clone());
        self.name = name;
    }

    /// The same bindings under a new id and name — every map a user adds
    /// starts from one that works, since an empty one would leave the page
    /// with no cursor and no click.
    pub fn copy(&self, id: &str, name: String, keys: &KeyNames) -> InputMap {
        let mut raw = self.raw.clone();
        raw.name = Some(name);
        InputMap::resolve(id, raw, keys)
    }

    /// A map that binds nothing, which is the whole pad passing through to the
    /// page — what an unbound source already means.
    pub fn passthrough(id: &str, name: String, keys: &KeyNames) -> InputMap {
        let raw = RawInputMap {
            name: Some(name),
            ..RawInputMap::default()
        };
        InputMap::resolve(id, raw, keys)
    }
}

#[cfg(test)]
mod tests {
    use super::store::{parse_built_in, BUILT_IN};
    use super::*;
    use keyboard_types::NamedKey;

    fn resolve(text: &str) -> InputMap {
        let raw: RawInputMap = toml::from_str(text).expect("valid map");
        InputMap::resolve("test", raw, &KeyNames::new())
    }

    /// The built-ins ship in the binary, so a typo in one is a startup panic —
    /// it has to fail here instead. Each keeps the pointer path or binds
    /// nothing at all: `pad` is raw so a game reads the sticks itself.
    #[test]
    fn every_built_in_keeps_the_pointer_or_is_fully_raw() {
        let keys = KeyNames::new();
        for (id, text) in BUILT_IN {
            let map = InputMap::resolve(id, parse_built_in(id, text), &keys);
            assert!(!map.name.is_empty(), "`{id}` has no name");
            // Which source clicks and which stick points is the map's own
            // business; that it can click and point at all is not.
            let clicks = Pad::ALL
                .into_iter()
                .any(|pad| matches!(map.pad(None, pad), Some(Target::Click(_))));
            let points = Side::ALL
                .into_iter()
                .any(|side| matches!(map.stick(side), StickRole::Analog(Target::Cursor { .. })));
            // Raw = nothing is withheld from the page: the Gamepad API sees
            // the whole pad, sticks included.
            let raw = Pad::ALL
                .into_iter()
                .all(|pad| matches!(map.pad(None, pad), None | Some(Target::Passthrough)))
                && Side::ALL.into_iter().all(|side| {
                    matches!(
                        map.stick(side),
                        StickRole::Unbound | StickRole::Analog(Target::Passthrough)
                    )
                })
                && map.keys.is_empty()
                && map.layers.is_empty();
            assert!(
                raw || (clicks && points),
                "`{id}` binds sources but cannot point and click"
            );
        }
    }

    /// A key the file binds reaches the runtime under the code SDL gives it,
    /// which is what the editor's rows are written against.
    #[test]
    fn the_key_table_resolves_to_sdl_codes() {
        let code = KeyNames::new().code("w").expect("SDL spells one key `w`");
        let map = resolve("[key]\nw = \"key.ArrowUp\"\n");
        assert!(matches!(map.key(None, code), Some(Target::Key(_))));
    }

    /// A key that names no device is a typo waiting to read as one of the
    /// words the format keeps for itself, so it is refused rather than guessed.
    #[test]
    fn a_target_that_names_no_device_is_refused() {
        let map = resolve(
            r#"
            [pad]
            a = "Space"
            b = "key.Space"
            "#,
        );
        assert_eq!(map.pad(None, Pad::A), None);
        assert!(map.pad(None, Pad::B).is_some());
    }

    /// A game branching on `e.code` gets nothing from `Unidentified`, and the
    /// name that derives a code is not every key's — Shift and Control need
    /// theirs said out loud, which is the trap a stock map must not ship.
    #[test]
    fn no_built_in_sends_a_key_the_page_cannot_identify() {
        let names = KeyNames::new();
        for (id, text) in BUILT_IN {
            let map = InputMap::resolve(id, parse_built_in(id, text), &names);
            let check = |target: Option<&Target>, whose: String| {
                if let Some(Target::Key(key)) = target {
                    assert_ne!(key.code, Code::Unidentified, "{whose}");
                }
            };
            for pad in Pad::ALL {
                check(map.pad(None, pad), format!("{id}.pad.{}", pad.name()));
            }
            for side in Side::ALL {
                let StickRole::Digital(dirs) = map.stick(side) else {
                    continue;
                };
                for dir in Dir::ALL {
                    let whose = format!("{id}.stick.{}.{}", side.name(), dir.name());
                    check(dirs[dir as usize].as_ref(), whose);
                }
            }
        }
    }

    /// The spellings that moved into the mouse namespace are refused where they
    /// stood, rather than resolving to nothing and leaving a stick dead.
    #[test]
    fn the_old_analog_spellings_are_refused() {
        let map = resolve(
            r#"
            [stick.left]
            analog = "key.cursor"
            [stick.right]
            analog = "mouse.scroll"
            "#,
        );
        assert_eq!(map.stick(Side::Left), &StickRole::Unbound);
        assert_eq!(
            map.stick(Side::Right),
            &StickRole::Analog(Target::Scroll { speed: 1.0 })
        );
    }

    /// Every mouse button has a route, and a held source steps the page or the
    /// cursor once per frame in the direction it names — down is positive both
    /// ways (the page's own `dy` reveals lower content, screen y grows down).
    #[test]
    fn the_mouse_targets_resolve_to_buttons_and_steps() {
        let map = resolve(
            r#"
            [pad]
            a = "mouse.left"
            b = "mouse.right"
            x = "mouse.middle"
            l1 = "mouse.scroll.down"
            r1 = "mouse.scroll.left"
            up = "mouse.cursor.up"
            right = "mouse.cursor.right"
            "#,
        );
        let button = |pad| match map.pad(None, pad) {
            Some(Target::Click(button)) => *button,
            other => panic!("{pad:?} resolved to {other:?}"),
        };
        assert_eq!(button(Pad::A), ClickButton::Left);
        assert_eq!(button(Pad::B), ClickButton::Right);
        assert_eq!(button(Pad::X), ClickButton::Middle);
        assert_eq!(
            map.pad(None, Pad::L1),
            Some(&Target::ScrollBy {
                x: 0.0,
                y: 1.0,
                speed: 1.0
            })
        );
        assert_eq!(
            map.pad(None, Pad::R1),
            Some(&Target::ScrollBy {
                x: -1.0,
                y: 0.0,
                speed: 1.0
            })
        );
        assert_eq!(
            map.pad(None, Pad::Up),
            Some(&Target::CursorBy {
                x: 0.0,
                y: -1.0,
                speed: 1.0
            })
        );
        assert_eq!(
            map.pad(None, Pad::Right),
            Some(&Target::CursorBy {
                x: 1.0,
                y: 0.0,
                speed: 1.0
            })
        );
    }

    /// A step is an edge's target: the whole stick has a vector of its own, and
    /// taking a step there would throw three of its four quadrants away.
    #[test]
    fn a_stick_refuses_a_step_and_keeps_the_whole_vector() {
        let map = resolve(
            r#"
            [stick.left]
            analog = "mouse.cursor.up"
            [stick.right]
            analog = { to = "mouse.cursor", speed = 2.0 }
            "#,
        );
        assert_eq!(map.stick(Side::Left), &StickRole::Unbound);
        assert_eq!(
            map.stick(Side::Right),
            &StickRole::Analog(Target::Cursor { speed: 2.0 })
        );
    }

    /// A map may deal the pad's own buttons out again, and give a key one —
    /// the page reads both through the Gamepad API.
    #[test]
    fn a_button_target_resolves_to_the_pad_it_names() {
        let map = resolve(
            r#"
            [pad]
            a = "pad.b"
            b = "pad.nosuchbutton"

            [key]
            w = "pad.l1"
            "#,
        );
        assert_eq!(map.pad(None, Pad::A), Some(&Target::Pad(Pad::B)));
        assert_eq!(map.pad(None, Pad::B), None);
        let code = KeyNames::new().code("w").expect("SDL spells one key `w`");
        assert_eq!(map.key(None, code), Some(&Target::Pad(Pad::L1)));
    }

    /// A stick is read whole, so a step in one direction says nothing about it.
    #[test]
    fn a_scroll_step_is_refused_on_a_whole_stick() {
        let map = resolve(
            r#"
            [stick.left]
            analog = "mouse.scroll.down"
            "#,
        );
        assert_eq!(map.stick(Side::Left), &StickRole::Unbound);
    }

    /// `code` is what a game branches on, so the common spellings must derive it
    /// without the file having to say so.
    #[test]
    fn a_bare_string_derives_the_code_a_game_reads() {
        let map = resolve(
            r#"
            [pad]
            a = "key.Space"
            b = "key.z"
            start = "key.Enter"
            "#,
        );
        let key = |pad| match map.pad(None, pad) {
            Some(Target::Key(k)) => k.clone(),
            other => panic!("{pad:?} resolved to {other:?}"),
        };
        assert_eq!(key(Pad::A).code, Code::Space);
        assert_eq!(key(Pad::A).key, Key::Character(" ".into()));
        assert_eq!(key(Pad::B).code, Code::KeyZ);
        assert_eq!(key(Pad::Start).code, Code::Enter);
    }

    /// The long form is for what the short one cannot say.
    #[test]
    fn the_table_form_carries_the_code_and_the_modifiers() {
        let map = resolve(
            r#"
            [pad]
            x = { to = "key.x", code = "KeyY", shift = true }
            "#,
        );
        let Some(Target::Key(key)) = map.pad(None, Pad::X) else {
            panic!("x is not a key");
        };
        assert_eq!(key.code, Code::KeyY);
        assert!(key.modifiers.contains(Modifiers::SHIFT));
    }

    /// Select carries the menu in every map — the one refusal that has to be
    /// loud, since a silent drop looks like a typo.
    #[test]
    fn select_is_refused_and_everything_else_survives_it() {
        let map = resolve(
            r#"
            [pad]
            select = "key.Escape"
            a = "key.Space"
            "#,
        );
        assert_eq!(map.pad(None, Pad::Select), None);
        assert!(map.pad(None, Pad::A).is_some());
    }

    #[test]
    fn a_stick_is_four_directions_or_one_vector() {
        let map = resolve(
            r#"
            [stick.left]
            up = "key.ArrowUp"
            [stick.right]
            analog = "mouse.cursor"
            "#,
        );
        let StickRole::Digital(dirs) = map.stick(Side::Left) else {
            panic!("the left stick is not digital");
        };
        assert!(dirs[Dir::Up as usize].is_some() && dirs[Dir::Down as usize].is_none());
        assert_eq!(
            map.stick(Side::Right),
            &StickRole::Analog(Target::Cursor { speed: 1.0 })
        );
    }

    /// A typo costs its own binding and nothing else.
    #[test]
    fn an_unknown_name_is_dropped_without_taking_the_map_with_it() {
        let map = resolve(
            r#"
            [pad]
            elbow = "key.Space"
            a = "key.NoSuchKey"
            b = "key.z"
            "#,
        );
        assert_eq!(map.pad(None, Pad::A), None);
        assert!(map.pad(None, Pad::B).is_some());
    }

    /// The point of a layer: the same button means two things, and the one the
    /// layer leaves alone still means what the base says.
    #[test]
    fn a_layer_overrides_what_it_names_and_falls_through_for_the_rest() {
        let map = resolve(
            r#"
            [pad]
            l2 = "layer:aim"
            a = "key.Space"
            b = "key.z"

            [layer.aim.pad]
            a = "key.Shift"
            "#,
        );
        assert_eq!(map.pad(None, Pad::L2), Some(&Target::Layer(0)));
        let named = |layer, pad| match map.pad(layer, pad) {
            Some(Target::Key(key)) => key.key.clone(),
            other => panic!("{pad:?} resolved to {other:?}"),
        };
        assert_eq!(named(None, Pad::A), Key::Character(" ".into()));
        assert_eq!(named(Some(0), Pad::A), Key::Named(NamedKey::Shift));
        // B is the base's in both, which is what makes a layer worth holding.
        assert_eq!(named(Some(0), Pad::B), named(None, Pad::B));
    }

    /// The file is one form or the other, so writing either has to take the
    /// other away: one holding both resolves as the directions, which would
    /// make the row just set a lie.
    #[test]
    fn a_stick_is_written_as_one_form_or_the_other() {
        let mut map = resolve(
            r#"
            [stick.left]
            analog = "mouse.cursor"
            "#,
        );
        assert!(!map.raw_stick_is_digital(Side::Left));
        map.set_raw_stick_arrows(Side::Left);
        assert!(map.raw_stick(Side::Left).is_none());
        assert!(map.raw_stick_is_digital(Side::Left));
        assert_eq!(
            map.raw_stick_dir(Side::Left, Dir::Up).map(RawTarget::text),
            Some("key.ArrowUp")
        );
        // And back: the directions go when the whole stick is given a target.
        let scroll = RawTarget::Short("scroll".to_string());
        map.set_raw_stick(Side::Left, Some(scroll));
        assert!(map.raw_stick_dir(Side::Left, Dir::Up).is_none());
        assert_eq!(
            map.raw_stick(Side::Left).map(RawTarget::text),
            Some("scroll")
        );
    }

    /// `none` on a whole stick is not `passthrough`: the editor offers both, so
    /// they have to resolve to different things.
    #[test]
    fn a_stick_told_to_send_nothing_is_not_the_same_as_one_left_alone() {
        let map = resolve(
            r#"
            [stick.left]
            analog = "none"
            [stick.right]
            analog = "passthrough"
            "#,
        );
        assert_eq!(map.stick(Side::Left), &StickRole::Analog(Target::None));
        assert_eq!(
            map.stick(Side::Right),
            &StickRole::Analog(Target::Passthrough)
        );
    }

    /// A copy is the only way to add a map, so it has to carry the whole
    /// mapping over — including what the editor cannot reach.
    #[test]
    fn a_copy_takes_the_bindings_and_the_new_name() {
        let map = resolve(
            r#"
            name = "Original"
            [pad]
            a = "key.Space"
            [stick.right]
            analog = "mouse.cursor"
            "#,
        );
        let copy = map.copy("my-game", "My game".to_string(), &KeyNames::new());
        assert_eq!(copy.id, "my-game");
        assert_eq!(copy.name, "My game");
        assert_eq!(copy.pad(None, Pad::A), map.pad(None, Pad::A));
        assert!(copy.stick(Side::Right).is_analog());
        // The copy is the user's, whatever it was copied from.
        assert!(!copy.builtin);
    }

    /// An id collision would shadow a map instead of adding one, so a name
    /// already spoken for has to land on a stem of its own.
    #[test]
    fn a_typed_name_becomes_a_free_file_stem() {
        let taken = ["vampire-survivors".to_string(), "keys".to_string()];
        assert_eq!(new_id("My Game!", &taken), "my-game");
        assert_eq!(new_id("Vampire Survivors", &taken), "vampire-survivors-2");
        assert_eq!(new_id("  ...  ", &taken), "map");
    }

    /// A layer that opens a layer is a knot to debug, and a name that is not
    /// there is a typo — both refused rather than half-applied.
    #[test]
    fn an_activator_needs_a_layer_that_exists_and_layers_hold_none() {
        let map = resolve(
            r#"
            [pad]
            l1 = "layer:nosuch"
            l2 = "layer:aim"

            [layer.aim.pad]
            x = "layer:aim"
            "#,
        );
        assert_eq!(map.pad(None, Pad::L1), None);
        assert_eq!(map.pad(None, Pad::L2), Some(&Target::Layer(0)));
        assert_eq!(map.pad(Some(0), Pad::X), None);
    }
}
