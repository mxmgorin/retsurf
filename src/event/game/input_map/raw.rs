//! The file as TOML spells it ([`RawTarget`] and friends), and its resolution
//! into an [`InputMap`]. Refusals are logged and dropped rather than failing
//! the file: a typo should cost one binding, not the map.

use super::store::is_built_in;
use super::{
    ClickButton, Dir, InputMap, KeyTarget, Layer, Side, StickRole, Target, ANALOG, KEY_PREFIX,
    LAYER_PREFIX, PAD_PREFIX,
};
use inputbind::sdl::KeyNames;
use inputbind::Pad;
use keyboard_types::{Code, Key, Modifiers, NamedKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

/// A target as written: a bare string for the common case, a table when the
/// `code`, a modifier or a speed has to be said out loud.
#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RawTarget {
    Short(String),
    Long {
        to: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        shift: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ctrl: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        alt: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        speed: Option<f32>,
    },
}

/// The modifier keys a key target holds down around itself.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct KeyMods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl RawTarget {
    /// How the file spells it, for the editor's rows.
    pub fn text(&self) -> &str {
        match self {
            RawTarget::Short(text) => text,
            RawTarget::Long { to, .. } => to,
        }
    }

    pub fn mods(&self) -> KeyMods {
        match self {
            RawTarget::Short(_) => KeyMods::default(),
            &RawTarget::Long {
                shift, ctrl, alt, ..
            } => KeyMods { shift, ctrl, alt },
        }
    }

    /// The speed a whole-stick mouse target moves at; `None` for the default.
    pub fn speed(&self) -> Option<f32> {
        match self {
            RawTarget::Short(_) => None,
            RawTarget::Long { speed, .. } => *speed,
        }
    }

    /// The same target at `speed` instead.
    pub fn with_speed(&self, speed: Option<f32>) -> RawTarget {
        match self {
            RawTarget::Short(to) => Self::long(None, KeyMods::default(), speed, to.clone()),
            RawTarget::Long { to, code, .. } => {
                Self::long(code.clone(), self.mods(), speed, to.clone())
            }
        }
    }

    /// `to` in place of what `old` sent, keeping whatever of `old` still applies:
    /// its modifiers for a key, its speed for a whole-stick mouse, its `code` for the same key.
    pub fn retarget(old: Option<&RawTarget>, to: String) -> RawTarget {
        let Some(RawTarget::Long {
            to: was,
            code,
            shift,
            ctrl,
            alt,
            speed,
        }) = old
        else {
            return RawTarget::Short(to);
        };
        let key = to.starts_with(KEY_PREFIX);
        Self::long(
            code.clone().filter(|_| *was == to),
            KeyMods {
                shift: *shift && key,
                ctrl: *ctrl && key,
                alt: *alt && key,
            },
            speed.filter(|_| [CURSOR, SCROLL].contains(&to.as_str())),
            to,
        )
    }

    /// The same target holding `mods` instead.
    pub fn with_mods(&self, mods: KeyMods) -> RawTarget {
        match self {
            RawTarget::Short(to) => Self::long(None, mods, None, to.clone()),
            RawTarget::Long {
                to, code, speed, ..
            } => Self::long(code.clone(), mods, *speed, to.clone()),
        }
    }

    /// The table form when it says anything beyond `to`, else the bare string.
    fn long(code: Option<String>, mods: KeyMods, speed: Option<f32>, to: String) -> RawTarget {
        if code.is_none() && mods == KeyMods::default() && speed.is_none() {
            return RawTarget::Short(to);
        }
        let KeyMods { shift, ctrl, alt } = mods;
        RawTarget::Long {
            to,
            code,
            shift,
            ctrl,
            alt,
            speed,
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub(super) struct RawLayer {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) pad: BTreeMap<String, RawTarget>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) key: BTreeMap<String, RawTarget>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) stick: BTreeMap<String, BTreeMap<String, RawTarget>>,
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub(super) struct RawInputMap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) pad: BTreeMap<String, RawTarget>,
    /// Keyed by stick (`left` / `right`), then by direction or `analog`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) stick: BTreeMap<String, BTreeMap<String, RawTarget>>,
    /// Physical keys, named as the editor's rows name them (`key.w`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) key: BTreeMap<String, RawTarget>,
    /// Alternate sets by name, each held open by whatever names it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) layer: BTreeMap<String, RawLayer>,
}

/// The two targets a stick drives as a whole, and the base a `.<direction>`
/// suffix turns into a per-frame step.
const CURSOR: &str = "mouse.cursor";
const SCROLL: &str = "mouse.scroll";

/// The unit step `text` names, if it is `base` with a direction after it.
fn step_after(text: &str, base: &str) -> Option<(f32, f32)> {
    let dir = text.strip_prefix(base)?.strip_prefix('.')?;
    Dir::parse(dir).map(Dir::step)
}

/// Resolve one written target against the layers the file declares. `None` is a
/// refusal, already logged.
fn parse_target(raw: &RawTarget, whose: &str, layers: &[String]) -> Option<Target> {
    let (text, code, modifiers, speed) = match raw {
        RawTarget::Short(text) => (text.as_str(), None, Modifiers::empty(), None),
        RawTarget::Long {
            to,
            code,
            shift,
            ctrl,
            alt,
            speed,
        } => {
            let mut modifiers = Modifiers::empty();
            modifiers.set(Modifiers::SHIFT, *shift);
            modifiers.set(Modifiers::CONTROL, *ctrl);
            modifiers.set(Modifiers::ALT, *alt);
            (to.as_str(), code.as_deref(), modifiers, *speed)
        }
    };
    let speed = speed.unwrap_or(1.0);
    if let Some(name) = text.strip_prefix(LAYER_PREFIX) {
        return match layers.iter().position(|l| l == name) {
            Some(index) => Some(Target::Layer(index)),
            None => {
                log::warn!("input map: `{whose}` holds no layer `{name}`; ignored");
                None
            }
        };
    }
    match text {
        "passthrough" => return Some(Target::Passthrough),
        "none" => return Some(Target::None),
        CURSOR => return Some(Target::Cursor { speed }),
        SCROLL => return Some(Target::Scroll { speed }),
        "mouse.left" => return Some(Target::Click(ClickButton::Left)),
        "mouse.right" => return Some(Target::Click(ClickButton::Right)),
        "mouse.middle" => return Some(Target::Click(ClickButton::Middle)),
        "cursor" | "scroll" => {
            log::warn!("input map: `{whose}` — `{text}` is spelled `mouse.{text}` now");
            return None;
        }
        _ => {}
    }
    // Named with a direction, either takes an edge instead of a stick: a step
    // per frame for as long as the source is held.
    if let Some((x, y)) = step_after(text, SCROLL) {
        return Some(Target::ScrollBy { x, y, speed });
    }
    if let Some((x, y)) = step_after(text, CURSOR) {
        return Some(Target::CursorBy { x, y, speed });
    }
    if let Some(name) = text.strip_prefix(PAD_PREFIX) {
        return match Pad::parse(name) {
            Some(pad) => Some(Target::Pad(pad)),
            None => {
                log::warn!("input map: `{whose}` names no pad button `{name}`; ignored");
                None
            }
        };
    }
    // Every target names its device, so a key cannot be read as a typo of one
    // of the words above.
    let Some(name) = text.strip_prefix(KEY_PREFIX) else {
        log::warn!("input map: `{whose}` — a key is spelled `{KEY_PREFIX}{text}`");
        return None;
    };
    let key = parse_key(name, whose)?;
    // An explicit `code` wins; otherwise the standard spells most keys the same
    // in both, and a game reading `e.code` gets nothing from Unidentified.
    let code = match code {
        Some(text) => Code::from_str(text).unwrap_or_else(|_| {
            log::warn!("input map: `{whose}` names no known code `{text}`");
            Code::Unidentified
        }),
        None => derive_code(name, &key, whose),
    };
    // A browser reports a shifted letter as the capital.
    let key = match key {
        Key::Character(c) if modifiers.contains(Modifiers::SHIFT) => {
            Key::Character(c.to_uppercase())
        }
        key => key,
    };
    Some(Target::Key(KeyTarget {
        key,
        code,
        modifiers,
    }))
}

/// A written key: one character, the friendly `Space`, or a `NamedKey` spelling.
fn parse_key(text: &str, whose: &str) -> Option<Key> {
    // Space is a character key in the standard, but nobody writes " " in a file.
    if text.eq_ignore_ascii_case("space") {
        return Some(Key::Character(" ".to_string()));
    }
    let mut chars = text.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(Key::Character(c.to_string()));
    }
    match NamedKey::from_str(text) {
        Ok(named) => Some(Key::Named(named)),
        Err(_) => {
            log::warn!("input map: `{whose}` names no key `{text}`; ignored");
            None
        }
    }
}

/// The `code` for a key whose spelling the standard shares, else a warning: a
/// game branching on `e.code` would silently get nothing.
fn derive_code(text: &str, key: &Key, whose: &str) -> Code {
    if let Key::Character(c) = key {
        // Space's character is " ", whose own spelling is not a code name.
        let name = match c.as_str() {
            " " => "Space",
            _ => return crate::event::sdl2_servo::code_for_char(c.chars().next().unwrap_or(' ')),
        };
        return Code::from_str(name).unwrap_or(Code::Unidentified);
    }
    let code = crate::event::sdl2_servo::code_for_named(text);
    if code == Code::Unidentified {
        log::warn!("input map: `{whose}` needs an explicit `code`; games read it");
    }
    code
}

impl InputMap {
    /// Resolve a written map (see the module doc for the refusal policy).
    pub(super) fn resolve(id: &str, raw: RawInputMap, keys: &KeyNames) -> InputMap {
        // Names first: an activator in any table resolves to an index here.
        let names: Vec<String> = raw.layer.keys().cloned().collect();
        let pad = resolve_pad_table(id, "pad", &raw.pad, &names);

        let mut sticks = [StickRole::Unbound, StickRole::Unbound];
        for (side, role) in resolve_sticks(id, "stick", &raw.stick, &names) {
            sticks[side as usize] = role;
        }

        let resolved_keys = resolve_keys(id, "key", &raw.key, keys, &names);

        // A layer's own tables, in the order its names were collected. Layers
        // hold no activators: a set that opens another is a knot to debug.
        let layers = names
            .iter()
            .map(|name| {
                let raw_layer = &raw.layer[name];
                let mut sticks = [None, None];
                for (side, role) in
                    resolve_sticks(id, &format!("layer.{name}.stick"), &raw_layer.stick, &[])
                {
                    sticks[side as usize] = Some(role);
                }
                Layer {
                    pad: resolve_pad_table(id, &format!("layer.{name}.pad"), &raw_layer.pad, &[]),
                    keys: resolve_keys(id, &format!("layer.{name}.key"), &raw_layer.key, keys, &[]),
                    sticks,
                }
            })
            .collect::<Vec<Layer>>();

        // A stick that opened a layer taking that stick would hand itself
        // over to itself on every switch.
        for (side, role) in Side::ALL.into_iter().zip(&mut sticks) {
            let StickRole::Digital(dirs) = role else {
                continue;
            };
            for (dir, target) in Dir::ALL.into_iter().zip(dirs.iter_mut()) {
                let opens_stick_layer = matches!(target, Some(Target::Layer(i))
                    if layers[*i].sticks.iter().any(Option::is_some));
                if opens_stick_layer {
                    log::warn!(
                        "input map: `{id}.stick.{}.{}` opens a layer that remaps a stick; ignored",
                        side.name(),
                        dir.name()
                    );
                    *target = None;
                }
            }
        }

        InputMap {
            id: id.to_string(),
            name: raw.name.clone().unwrap_or_else(|| id.to_string()),
            builtin: is_built_in(id),
            file: false,
            fixed: false,
            pad,
            sticks,
            keys: resolved_keys,
            layers,
            raw,
        }
    }
}

/// One `[pad]` table, base or layer — the one spelling of its rules, so a rule
/// added here cannot miss a copy.
fn resolve_pad_table(
    id: &str,
    scope: &str,
    table: &BTreeMap<String, RawTarget>,
    layers: &[String],
) -> Vec<Option<Target>> {
    let mut pad = vec![None; Pad::COUNT];
    for (name, raw) in table {
        let whose = format!("{id}.{scope}.{name}");
        let Some(slot) = Pad::parse(name) else {
            log::warn!("input map: `{whose}` is not a pad; ignored");
            continue;
        };
        match parse_target(raw, &whose, layers) {
            Some(target) if target.is_analog() => {
                log::warn!("input map: `{whose}` is a button, not a stick");
            }
            Some(target) => pad[slot as usize] = Some(target),
            None => {}
        }
    }
    pad
}

/// One `[key]` table, base or layer: names resolved through SDL, sorted so
/// the runtime can binary-search them.
fn resolve_keys(
    id: &str,
    scope: &str,
    raw: &BTreeMap<String, RawTarget>,
    keys: &KeyNames,
    layers: &[String],
) -> Vec<(u32, Target)> {
    let mut resolved: Vec<(u32, Target)> = raw
        .iter()
        .filter_map(|(name, raw)| {
            let whose = format!("{id}.{scope}.{name}");
            let Some(code) = keys.code(name) else {
                log::warn!("input map: SDL has no key `{name}` (`{whose}`); ignored");
                return None;
            };
            let target = parse_target(raw, &whose, layers)?;
            if target.is_analog() {
                log::warn!("input map: `{whose}` is a key, not a stick");
                return None;
            }
            Some((code, target))
        })
        .collect();
    resolved.sort_by_key(|(code, _)| *code);
    resolved.dedup_by_key(|(code, _)| *code);
    resolved
}

/// One stick table: four directions, or `analog` for the whole stick. Both at
/// once is a contradiction — the directions win, since they are the specific ones.
/// One `stick` table, base or layer, keyed by side; `path` is where it sits
/// in the file, for the log.
fn resolve_sticks(
    id: &str,
    path: &str,
    tables: &BTreeMap<String, BTreeMap<String, RawTarget>>,
    layers: &[String],
) -> Vec<(Side, StickRole)> {
    tables
        .iter()
        .filter_map(|(name, table)| {
            let path = format!("{path}.{name}");
            let Some(side) = Side::parse(name) else {
                log::warn!("input map: `{id}.{path}` is not a stick; ignored");
                return None;
            };
            Some((side, resolve_stick(id, &path, table, layers)))
        })
        .collect()
}

fn resolve_stick(
    id: &str,
    path: &str,
    table: &BTreeMap<String, RawTarget>,
    layers: &[String],
) -> StickRole {
    let mut dirs: [Option<Target>; 4] = [None, None, None, None];
    let mut analog = None;
    for (key, raw) in table {
        let whose = format!("{id}.{path}.{key}");
        let Some(target) = parse_target(raw, &whose, layers) else {
            continue;
        };
        if key == ANALOG {
            if !target.is_analog() && target != Target::Passthrough && target != Target::None {
                log::warn!("input map: `{whose}` takes mouse.cursor, mouse.scroll or passthrough");
                continue;
            }
            analog = Some(target);
            continue;
        }
        let Some(dir) = Dir::parse(key) else {
            log::warn!("input map: `{whose}` is not a direction; ignored");
            continue;
        };
        if target.is_analog() {
            log::warn!("input map: `{whose}` is one direction, not the stick");
            continue;
        }
        dirs[dir as usize] = Some(target);
    }
    let has_dirs = dirs.iter().any(Option::is_some);
    match (has_dirs, analog) {
        (true, Some(_)) => {
            log::warn!("input map: `{id}.{path}` is both directions and analog");
            StickRole::Digital(Box::new(dirs))
        }
        (true, None) => StickRole::Digital(Box::new(dirs)),
        (false, Some(target)) => StickRole::Analog(target),
        (false, None) => StickRole::Unbound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shifted(to: &str, code: Option<&str>) -> RawTarget {
        let mods = KeyMods {
            shift: true,
            ..KeyMods::default()
        };
        let code = code.map(str::to_string);
        RawTarget::Short(to.to_string())
            .with_mods(mods)
            .with_code(code)
    }

    impl RawTarget {
        fn with_code(self, code: Option<String>) -> RawTarget {
            match self {
                RawTarget::Long {
                    to,
                    shift,
                    ctrl,
                    alt,
                    speed,
                    ..
                } => RawTarget::Long {
                    to,
                    code,
                    shift,
                    ctrl,
                    alt,
                    speed,
                },
                short => short,
            }
        }
    }

    #[test]
    fn a_new_key_keeps_the_modifiers_and_drops_the_old_code() {
        let old = shifted("key.1", Some("Digit1"));
        let new = RawTarget::retarget(Some(&old), "key.2".to_string());
        assert_eq!(new.text(), "key.2");
        assert!(new.mods().shift);
        assert!(matches!(new, RawTarget::Long { code: None, .. }));
    }

    #[test]
    fn a_target_that_is_no_key_sheds_the_modifiers() {
        let old = shifted("key.1", None);
        let new = RawTarget::retarget(Some(&old), "mouse.left".to_string());
        assert!(matches!(new, RawTarget::Short(ref to) if to == "mouse.left"));
    }

    #[test]
    fn clearing_the_last_modifier_writes_the_short_form() {
        let new = shifted("key.1", None).with_mods(KeyMods::default());
        assert!(matches!(new, RawTarget::Short(ref to) if to == "key.1"));
    }

    #[test]
    fn the_default_speed_writes_the_short_form() {
        let slow = RawTarget::Short(CURSOR.to_string()).with_speed(Some(0.35));
        assert_eq!(slow.speed(), Some(0.35));
        assert!(matches!(slow.with_speed(None), RawTarget::Short(ref to) if to == CURSOR));
    }

    #[test]
    fn a_stick_keeps_its_speed_across_a_retarget() {
        let old = RawTarget::Long {
            to: CURSOR.to_string(),
            code: None,
            shift: false,
            ctrl: false,
            alt: false,
            speed: Some(0.35),
        };
        let new = RawTarget::retarget(Some(&old), SCROLL.to_string());
        assert!(matches!(new, RawTarget::Long { speed: Some(_), .. }));
    }
}
