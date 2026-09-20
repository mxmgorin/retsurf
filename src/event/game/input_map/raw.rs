//! The file as TOML spells it ([`RawTarget`] and friends), and its resolution
//! into an [`InputMap`]. Refusals are logged and dropped rather than failing
//! the file: a typo should cost one binding, not the map.

use super::store::is_built_in;
use super::{Dir, InputMap, KeyTarget, Layer, Side, StickRole, Target, ANALOG, KEY_PREFIX};
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

impl RawTarget {
    /// How the file spells it, for the editor's rows.
    pub fn text(&self) -> &str {
        match self {
            RawTarget::Short(text) => text,
            RawTarget::Long { to, .. } => to,
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub(super) struct RawLayer {
    pad: BTreeMap<String, RawTarget>,
    key: BTreeMap<String, RawTarget>,
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
    if let Some(name) = text.strip_prefix("layer:") {
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
        "mouse.cursor" => return Some(Target::Cursor { speed }),
        "mouse.scroll" => return Some(Target::Scroll { speed }),
        "mouse.left" => return Some(Target::Click),
        // The namespace is open, but only the left button has a route: the
        // router's Confirm intent carries no button of its own.
        "mouse.right" | "mouse.middle" => {
            log::warn!("input map: `{whose}` — only `mouse.left` has a route");
            return None;
        }
        "cursor" | "scroll" => {
            log::warn!("input map: `{whose}` — `{text}` is spelled `mouse.{text}` now");
            return None;
        }
        _ => {}
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
        for (name, table) in &raw.stick {
            let Some(side) = Side::parse(name) else {
                log::warn!("input map: `{id}.stick.{name}` is not a stick; ignored");
                continue;
            };
            sticks[side as usize] = resolve_stick(id, name, table, &names);
        }

        let resolved_keys = resolve_keys(id, "key", &raw.key, keys, &names);

        // A layer's own tables, in the order its names were collected. Layers
        // hold no activators: a set that opens another is a knot to debug.
        let layers = names
            .iter()
            .map(|name| {
                let raw_layer = &raw.layer[name];
                Layer {
                    pad: resolve_pad_table(id, &format!("layer.{name}.pad"), &raw_layer.pad, &[]),
                    keys: resolve_keys(id, &format!("layer.{name}.key"), &raw_layer.key, keys, &[]),
                }
            })
            .collect();

        InputMap {
            id: id.to_string(),
            name: raw.name.clone().unwrap_or_else(|| id.to_string()),
            builtin: is_built_in(id),
            file: false,
            pad,
            sticks,
            keys: resolved_keys,
            layers,
            raw,
        }
    }
}

/// One `[pad]` table, base or layer — the one spelling of its rules, so a
/// refusal added here cannot miss a copy.
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
        // Select carries the menu in every map, so it is never the game's —
        // refused out loud rather than dropped silently.
        if slot == Pad::Select {
            log::warn!("input map: `{whose}` is reserved for the Game Mode menu");
            continue;
        }
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
fn resolve_stick(
    id: &str,
    name: &str,
    table: &BTreeMap<String, RawTarget>,
    layers: &[String],
) -> StickRole {
    let mut dirs: [Option<Target>; 4] = [None, None, None, None];
    let mut analog = None;
    for (key, raw) in table {
        let whose = format!("{id}.stick.{name}.{key}");
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
            log::warn!("input map: `{id}.stick.{name}` is both directions and analog");
            StickRole::Digital(Box::new(dirs))
        }
        (true, None) => StickRole::Digital(Box::new(dirs)),
        (false, Some(target)) => StickRole::Analog(target),
        (false, None) => StickRole::Unbound,
    }
}
