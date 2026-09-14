//! Game Mode's profiles: what each pad, stick direction and key sends to the
//! page while the mode is on. The built-ins live here in code and are always
//! offered; a `profiles/<id>.toml` in the data dir under a built-in's id
//! replaces it, and deleting that file is what "reset to default" means.
//!
//! ```toml
//! name = "Keyboard keys"
//!
//! [pad]                  # buttons and the D-pad, by inputbind's names
//! a = "Space"
//! b = "z"
//! r2 = "click"
//! l2 = "passthrough"     # reaches the page as the gamepad button it is
//!
//! [stick.left]           # four directions, through the dead zone
//! up = "ArrowUp"
//! [stick.right]
//! analog = "cursor"      # or "scroll" — the whole stick, not a direction
//!
//! [keyboard]             # physical keys, resolved after the pad keymap
//! w = "ArrowUp"
//! ```

use crate::config;
use inputbind::sdl::KeyNames;
use inputbind::Pad;
use keyboard_types::{Code, Key, Modifiers, NamedKey};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::str::FromStr;

/// Where the per-profile files live, under the user data dir.
const PROFILE_DIR: &str = "profiles";

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

    fn parse(name: &str) -> Option<Dir> {
        Some(match name {
            "up" => Dir::Up,
            "down" => Dir::Down,
            "left" => Dir::Left,
            "right" => Dir::Right,
            _ => return None,
        })
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

/// What a source does while the mode is on.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    Key(KeyTarget),
    /// The left mouse button at the cursor — the one the router's Confirm
    /// intent carries, and the path measured on hardware.
    Click,
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
    /// Consumed and dropped: the source is inert while the profile is active.
    None,
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

#[derive(Clone)]
pub struct Profile {
    /// The file stem, or the built-in's id; `[game_mode] profile` names this.
    pub id: String,
    /// What the menu shows.
    pub name: String,
    pad: Vec<Option<Target>>,
    /// Left, then right.
    sticks: [StickRole; 2],
    /// Physical keys by SDL keycode, sorted for lookup.
    keys: Vec<(u32, Target)>,
}

impl Profile {
    pub fn pad(&self, pad: Pad) -> Option<&Target> {
        self.pad.get(pad as usize).and_then(Option::as_ref)
    }

    pub fn stick(&self, right: bool) -> &StickRole {
        &self.sticks[usize::from(right)]
    }

    pub fn key(&self, code: u32) -> Option<&Target> {
        self.keys
            .binary_search_by_key(&code, |(c, _)| *c)
            .ok()
            .map(|i| &self.keys[i].1)
    }
}

// --- the file, as TOML spells it ---

/// A target as written: a bare string for the common case, a table when the
/// `code`, a modifier or a speed has to be said out loud.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawTarget {
    Short(String),
    Long {
        to: String,
        code: Option<String>,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        speed: Option<f32>,
    },
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProfile {
    name: Option<String>,
    pad: BTreeMap<String, RawTarget>,
    /// Keyed by stick (`left` / `right`), then by direction or `analog`.
    stick: BTreeMap<String, BTreeMap<String, RawTarget>>,
    keyboard: BTreeMap<String, RawTarget>,
}

/// Resolve one written target. `None` is a refusal, already logged.
fn parse_target(raw: &RawTarget, whose: &str) -> Option<Target> {
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
    match text {
        "passthrough" => return Some(Target::Passthrough),
        "none" => return Some(Target::None),
        "cursor" => return Some(Target::Cursor { speed }),
        "scroll" => return Some(Target::Scroll { speed }),
        "click" => return Some(Target::Click),
        _ => {}
    }
    let key = parse_key(text, whose)?;
    // An explicit `code` wins; otherwise the standard spells most keys the same
    // in both, and a game reading `e.code` gets nothing from Unidentified.
    let code = match code {
        Some(text) => Code::from_str(text).unwrap_or_else(|_| {
            log::warn!("game profile: `{whose}` names no known code `{text}`");
            Code::Unidentified
        }),
        None => derive_code(text, &key, whose),
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
            log::warn!("game profile: `{whose}` names no key `{text}`; ignored");
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
    Code::from_str(text).unwrap_or_else(|_| {
        log::warn!("game profile: `{whose}` needs an explicit `code`; games read it");
        Code::Unidentified
    })
}

impl Profile {
    /// Resolve a written profile. Refusals are logged and dropped rather than
    /// failing the file: a typo should cost one binding, not the profile.
    fn resolve(id: &str, raw: RawProfile, keys: &KeyNames) -> Profile {
        let mut pad = vec![None; Pad::COUNT];
        for (name, raw) in &raw.pad {
            let whose = format!("{id}.pad.{name}");
            let Some(slot) = Pad::parse(name) else {
                log::warn!("game profile: `{whose}` is not a pad; ignored");
                continue;
            };
            // Select carries the menu in every profile, so it is never the
            // game's — refused out loud rather than dropped silently.
            if slot == Pad::Select {
                log::warn!("game profile: `{whose}` is reserved for the Game Mode menu");
                continue;
            }
            if let Some(target) = parse_target(raw, &whose) {
                if target.is_analog() {
                    log::warn!("game profile: `{whose}` is a button, not a stick");
                    continue;
                }
                pad[slot as usize] = Some(target);
            }
        }

        let mut sticks = [StickRole::Unbound, StickRole::Unbound];
        for (name, table) in &raw.stick {
            let index = match name.as_str() {
                "left" => 0,
                "right" => 1,
                _ => {
                    log::warn!("game profile: `{id}.stick.{name}` is not a stick; ignored");
                    continue;
                }
            };
            sticks[index] = resolve_stick(id, name, table);
        }

        let mut resolved_keys: Vec<(u32, Target)> = raw
            .keyboard
            .iter()
            .filter_map(|(name, raw)| {
                let whose = format!("{id}.keyboard.{name}");
                let Some(code) = keys.code(name) else {
                    log::warn!("game profile: SDL has no key `{name}` (`{whose}`); ignored");
                    return None;
                };
                let target = parse_target(raw, &whose)?;
                if target.is_analog() {
                    log::warn!("game profile: `{whose}` is a key, not a stick");
                    return None;
                }
                Some((code, target))
            })
            .collect();
        resolved_keys.sort_by_key(|(code, _)| *code);
        resolved_keys.dedup_by_key(|(code, _)| *code);

        Profile {
            id: id.to_string(),
            name: raw.name.unwrap_or_else(|| id.to_string()),
            pad,
            sticks,
            keys: resolved_keys,
        }
    }
}

/// One stick table: four directions, or `analog` for the whole stick. Both at
/// once is a contradiction — the directions win, since they are the specific ones.
fn resolve_stick(id: &str, name: &str, table: &BTreeMap<String, RawTarget>) -> StickRole {
    let mut dirs: [Option<Target>; 4] = [None, None, None, None];
    let mut analog = None;
    for (key, raw) in table {
        let whose = format!("{id}.stick.{name}.{key}");
        let Some(target) = parse_target(raw, &whose) else {
            continue;
        };
        if key == "analog" {
            if !target.is_analog() && target != Target::Passthrough && target != Target::None {
                log::warn!("game profile: `{whose}` takes cursor, scroll or passthrough");
                continue;
            }
            analog = Some(target);
            continue;
        }
        let Some(dir) = Dir::parse(key) else {
            log::warn!("game profile: `{whose}` is not a direction; ignored");
            continue;
        };
        if target.is_analog() {
            log::warn!("game profile: `{whose}` is one direction, not the stick");
            continue;
        }
        dirs[dir as usize] = Some(target);
    }
    let has_dirs = dirs.iter().any(Option::is_some);
    match (has_dirs, analog) {
        (true, Some(_)) => {
            log::warn!("game profile: `{id}.stick.{name}` is both directions and analog");
            StickRole::Digital(Box::new(dirs))
        }
        (true, None) => StickRole::Digital(Box::new(dirs)),
        (false, Some(target)) => StickRole::Analog(target),
        (false, None) => StickRole::Unbound,
    }
}

// --- built-ins ---

/// The stock profiles, in the order the menu cycles them. They live in code, so
/// a release that adds one offers it to everyone; a file under the same id
/// replaces it, and deleting that file restores this.
const BUILT_IN: [(&str, &str); 2] = [("keys", KEYS_PROFILE), ("pad", PAD_PROFILE)];

/// The retro convention (arrows + z/x/c + Space/Enter) that PICO-8 exports and
/// js13k entries share, so most of itch.io plays with no profile edit at all.
const KEYS_PROFILE: &str = include_str!("../../resources/profiles/keys.toml");

/// The pad reaches the page raw, for games that read the Gamepad API themselves.
const PAD_PROFILE: &str = include_str!("../../resources/profiles/pad.toml");

/// Every profile this run offers: the built-ins, each replaced by a
/// `profiles/<id>.toml` that shadows it, plus whatever other files are there.
pub fn load_all(keys: &KeyNames) -> Vec<Profile> {
    let mut files = read_dir();
    let mut profiles: Vec<Profile> = BUILT_IN
        .iter()
        .map(|(id, text)| {
            let raw = files
                .remove(*id)
                .unwrap_or_else(|| parse_built_in(id, text));
            Profile::resolve(id, raw, keys)
        })
        .collect();
    // Whatever else the user put there, in a stable order.
    let mut extra: Vec<(String, RawProfile)> = files.into_iter().collect();
    extra.sort_by(|(a, _), (b, _)| a.cmp(b));
    profiles.extend(
        extra
            .into_iter()
            .map(|(id, raw)| Profile::resolve(&id, raw, keys)),
    );
    profiles
}

/// The profile `id` names, or the first — the built-in `keys` unless a file
/// shadows it, so an unknown id in the config is never a dead mode.
pub fn pick<'a>(profiles: &'a [Profile], id: &str) -> &'a Profile {
    profiles
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| profiles.first().expect("the built-ins are always offered"))
}

/// A built-in's own text, which a test parses for every one of them.
fn parse_built_in(id: &str, text: &str) -> RawProfile {
    toml::from_str(text).unwrap_or_else(|e| panic!("built-in profile `{id}` is invalid: {e}"))
}

/// Read every `profiles/*.toml`, keyed by file stem. A malformed file is logged
/// and skipped, like a malformed `bindings.toml`.
fn read_dir() -> BTreeMap<String, RawProfile> {
    let dir = format!("{}{PROFILE_DIR}", config::data_dir());
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match toml::from_str::<RawProfile>(&text) {
            Ok(raw) => {
                log::info!("game profile: loaded `{id}` from `{}`", path.display());
                out.insert(id.to_string(), raw);
            }
            Err(e) => log::error!("game profile `{}` is invalid: {e}; ignored", path.display()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(text: &str) -> Profile {
        let raw: RawProfile = toml::from_str(text).expect("valid profile");
        Profile::resolve("test", raw, &KeyNames::new())
    }

    /// The built-ins ship in the binary, so a typo in one is a startup panic —
    /// it has to fail here instead.
    #[test]
    fn every_built_in_resolves() {
        let keys = KeyNames::new();
        for (id, text) in BUILT_IN {
            let profile = Profile::resolve(id, parse_built_in(id, text), &keys);
            assert!(!profile.name.is_empty(), "`{id}` has no name");
        }
    }

    /// `code` is what a game branches on, so the common spellings must derive it
    /// without the file having to say so.
    #[test]
    fn a_bare_string_derives_the_code_a_game_reads() {
        let profile = resolve(
            r#"
            [pad]
            a = "Space"
            b = "z"
            start = "Enter"
            "#,
        );
        let key = |pad| match profile.pad(pad) {
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
        let profile = resolve(
            r#"
            [pad]
            x = { to = "x", code = "KeyY", shift = true }
            "#,
        );
        let Some(Target::Key(key)) = profile.pad(Pad::X) else {
            panic!("x is not a key");
        };
        assert_eq!(key.code, Code::KeyY);
        assert!(key.modifiers.contains(Modifiers::SHIFT));
    }

    /// Select carries the menu in every profile — the one refusal that has to be
    /// loud, since a silent drop looks like a typo.
    #[test]
    fn select_is_refused_and_everything_else_survives_it() {
        let profile = resolve(
            r#"
            [pad]
            select = "Escape"
            a = "Space"
            "#,
        );
        assert_eq!(profile.pad(Pad::Select), None);
        assert!(profile.pad(Pad::A).is_some());
    }

    #[test]
    fn a_stick_is_four_directions_or_one_vector() {
        let profile = resolve(
            r#"
            [stick.left]
            up = "ArrowUp"
            [stick.right]
            analog = "cursor"
            "#,
        );
        let StickRole::Digital(dirs) = profile.stick(false) else {
            panic!("the left stick is not digital");
        };
        assert!(dirs[Dir::Up as usize].is_some() && dirs[Dir::Down as usize].is_none());
        assert_eq!(
            profile.stick(true),
            &StickRole::Analog(Target::Cursor { speed: 1.0 })
        );
    }

    /// A typo costs its own binding and nothing else.
    #[test]
    fn an_unknown_name_is_dropped_without_taking_the_profile_with_it() {
        let profile = resolve(
            r#"
            [pad]
            elbow = "Space"
            a = "NoSuchKey"
            b = "z"
            "#,
        );
        assert_eq!(profile.pad(Pad::A), None);
        assert!(profile.pad(Pad::B).is_some());
    }
}
