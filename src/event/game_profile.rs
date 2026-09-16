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
//! r2 = "mouse.left"
//! l2 = "passthrough"     # reaches the page as the gamepad button it is
//!
//! [stick.left]           # four directions, through the dead zone
//! up = "ArrowUp"
//! [stick.right]
//! analog = "cursor"      # or "scroll" — the whole stick, not a direction
//!
//! [keyboard]             # physical keys, resolved after the pad keymap
//! w = "ArrowUp"
//!
//! [layer.aim.pad]        # while `l2 = "layer:aim"` is held
//! a = "Shift"
//! ```

use crate::config;
use inputbind::sdl::KeyNames;
use inputbind::Pad;
use keyboard_types::{Code, Key, Modifiers, NamedKey};
use serde::{Deserialize, Serialize};
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
    /// intent carries, and the path measured on hardware. Spelled `mouse.left`,
    /// so the other buttons have a name to arrive under.
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
pub struct Profile {
    /// The file stem, or the built-in's id; `[game_mode] profile` names this.
    pub id: String,
    /// What the menu shows.
    pub name: String,
    /// Whether the binary carries this id, so deleting its file restores the
    /// original rather than removing the profile.
    pub builtin: bool,
    /// Whether `profiles/<id>.toml` is there — what deleting removes, and the
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
    raw: RawProfile,
}

impl Profile {
    /// What a pad sends, under the held layer if one names it — a button the
    /// layer leaves alone falls through to the base rather than going inert.
    pub fn pad(&self, layer: Option<usize>, pad: Pad) -> Option<&Target> {
        let from_layer = layer
            .and_then(|i| self.layers.get(i))
            .and_then(|l| l.pad.get(pad as usize))
            .and_then(Option::as_ref);
        from_layer.or_else(|| self.pad.get(pad as usize).and_then(Option::as_ref))
    }

    pub fn stick(&self, right: bool) -> &StickRole {
        &self.sticks[usize::from(right)]
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

// --- the file, as TOML spells it ---

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
struct RawLayer {
    pad: BTreeMap<String, RawTarget>,
    keyboard: BTreeMap<String, RawTarget>,
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default)]
struct RawProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pad: BTreeMap<String, RawTarget>,
    /// Keyed by stick (`left` / `right`), then by direction or `analog`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    stick: BTreeMap<String, BTreeMap<String, RawTarget>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    keyboard: BTreeMap<String, RawTarget>,
    /// Alternate sets by name, each held open by whatever names it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    layer: BTreeMap<String, RawLayer>,
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
                log::warn!("game profile: `{whose}` holds no layer `{name}`; ignored");
                None
            }
        };
    }
    match text {
        "passthrough" => return Some(Target::Passthrough),
        "none" => return Some(Target::None),
        "cursor" => return Some(Target::Cursor { speed }),
        "scroll" => return Some(Target::Scroll { speed }),
        "mouse.left" => return Some(Target::Click),
        // The namespace is open, but only the left button has a route: the
        // router's Confirm intent carries no button of its own.
        "mouse.right" | "mouse.middle" => {
            log::warn!("game profile: `{whose}` — only `mouse.left` has a route");
            return None;
        }
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
        // Names first: an activator in any table resolves to an index here.
        let names: Vec<String> = raw.layer.keys().cloned().collect();
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
            if let Some(target) = parse_target(raw, &whose, &names) {
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
            sticks[index] = resolve_stick(id, name, table, &names);
        }

        let resolved_keys = resolve_keys(id, "keyboard", &raw.keyboard, keys, &names);

        // A layer's own tables, in the order its names were collected. Layers
        // hold no activators: a set that opens another is a knot to debug, not
        // a feature anyone asked for.
        let layers = names
            .iter()
            .map(|name| {
                let raw_layer = &raw.layer[name];
                let mut pad = vec![None; Pad::COUNT];
                for (source, raw) in &raw_layer.pad {
                    let whose = format!("{id}.layer.{name}.pad.{source}");
                    let Some(slot) = Pad::parse(source) else {
                        log::warn!("game profile: `{whose}` is not a pad; ignored");
                        continue;
                    };
                    if slot == Pad::Select {
                        log::warn!("game profile: `{whose}` is reserved for the Game Mode menu");
                        continue;
                    }
                    match parse_target(raw, &whose, &[]) {
                        Some(target) if target.is_analog() => {
                            log::warn!("game profile: `{whose}` is a button, not a stick");
                        }
                        Some(target) => pad[slot as usize] = Some(target),
                        None => {}
                    }
                }
                let scope = format!("layer.{name}.keyboard");
                Layer {
                    pad,
                    keys: resolve_keys(id, &scope, &raw_layer.keyboard, keys, &[]),
                }
            })
            .collect();

        Profile {
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

    /// Rename what the menu shows. The id stays: it is the file's stem, and
    /// `[game_mode] profile` names it.
    pub fn set_name(&mut self, name: String) {
        self.raw.name = Some(name.clone());
        self.name = name;
    }

    /// The same bindings under a new id and name — every profile a user adds
    /// starts from one that works, since an empty one would leave the page
    /// with no cursor and no click.
    pub fn copy(&self, id: &str, name: String, keys: &KeyNames) -> Profile {
        let mut raw = self.raw.clone();
        raw.name = Some(name);
        Profile::resolve(id, raw, keys)
    }

    /// Write the profile to `profiles/<id>.toml`, which is also how a built-in
    /// is replaced. Returns the re-resolved profile, so the edit takes effect
    /// without a restart.
    pub fn save(&self, keys: &KeyNames) -> Profile {
        let mut saved = Profile::resolve(&self.id, self.raw.clone(), keys);
        saved.file = write_file(&self.id, &self.raw);
        saved
    }

    /// Remove `profiles/<id>.toml`. For a built-in that is reset to default —
    /// the binary's own text comes back; for any other profile it is deletion.
    pub fn delete(&self) -> bool {
        let path = profile_path(&self.id);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                log::info!("game profile: removed `{path}`");
                true
            }
            Err(e) => {
                log::warn!("game profile: could not remove `{path}`: {e}");
                false
            }
        }
    }
}

/// Where a profile of this id is read from and written to.
fn profile_path(id: &str) -> String {
    format!("{}{PROFILE_DIR}/{id}.toml", config::data_dir())
}

/// Write one profile's file, reporting whether it is now on disk.
fn write_file(id: &str, raw: &RawProfile) -> bool {
    let text = match toml::to_string_pretty(raw) {
        Ok(text) => text,
        Err(e) => {
            log::warn!("game profile: could not serialize `{id}`: {e}");
            return false;
        }
    };
    let path = profile_path(id);
    let _ = std::fs::create_dir_all(format!("{}{PROFILE_DIR}", config::data_dir()));
    match std::fs::write(&path, text) {
        Ok(()) => {
            log::info!("game profile: wrote `{path}`");
            true
        }
        Err(e) => {
            log::warn!("game profile: could not write `{path}`: {e}");
            false
        }
    }
}

/// A file stem for a typed name: lowercased, one dash per run of anything
/// else, and never one of `taken` — an id collision would shadow a profile
/// instead of adding one.
pub fn new_id(name: &str, taken: &[String]) -> String {
    let slug: String = name
        .chars()
        .map(|c| match c.is_alphanumeric() {
            true => c.to_lowercase().next().unwrap_or(c),
            false => '-',
        })
        .collect();
    let base: String = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    // A name of nothing but punctuation still needs a stem to live under.
    let base = match base.is_empty() {
        true => "profile".to_string(),
        false => base,
    };
    if !taken.contains(&base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// One `[keyboard]` table, base or layer: names resolved through SDL, sorted so
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
                log::warn!("game profile: SDL has no key `{name}` (`{whose}`); ignored");
                return None;
            };
            let target = parse_target(raw, &whose, layers)?;
            if target.is_analog() {
                log::warn!("game profile: `{whose}` is a key, not a stick");
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
            let (raw, file) = match files.remove(*id) {
                Some(raw) => (raw, true),
                None => (parse_built_in(id, text), false),
            };
            let mut profile = Profile::resolve(id, raw, keys);
            profile.file = file;
            profile
        })
        .collect();
    // Whatever else the user put there, in a stable order.
    let mut extra: Vec<(String, RawProfile)> = files.into_iter().collect();
    extra.sort_by(|(a, _), (b, _)| a.cmp(b));
    profiles.extend(extra.into_iter().map(|(id, raw)| {
        let mut profile = Profile::resolve(&id, raw, keys);
        profile.file = true;
        profile
    }));
    profiles
}

/// Whether the binary carries a profile of this id.
fn is_built_in(id: &str) -> bool {
    BUILT_IN.iter().any(|(built_in, _)| *built_in == id)
}

/// The built-in `id` as the binary carries it — what deleting its file gives
/// back.
pub fn built_in(id: &str, keys: &KeyNames) -> Option<Profile> {
    BUILT_IN
        .iter()
        .find(|(built_in, _)| *built_in == id)
        .map(|(id, text)| Profile::resolve(id, parse_built_in(id, text), keys))
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
    /// it has to fail here instead. Both must also keep the pointer path: it is
    /// the one way of playing this milestone has measured on hardware, and a
    /// profile that drops it ships a regression against that.
    #[test]
    fn every_built_in_resolves_and_keeps_the_pointer() {
        let keys = KeyNames::new();
        for (id, text) in BUILT_IN {
            let profile = Profile::resolve(id, parse_built_in(id, text), &keys);
            assert!(!profile.name.is_empty(), "`{id}` has no name");
            assert_eq!(profile.pad(None, Pad::R2), Some(&Target::Click), "`{id}`");
            assert!(profile.stick(true).is_analog(), "`{id}` has no cursor");
        }
    }

    /// The namespace is open but the route is not, so the other buttons have to
    /// be refused out loud rather than resolving to the left one.
    #[test]
    fn only_the_left_mouse_button_resolves() {
        let profile = resolve(
            r#"
            [pad]
            a = "mouse.left"
            b = "mouse.right"
            "#,
        );
        assert_eq!(profile.pad(None, Pad::A), Some(&Target::Click));
        assert_eq!(profile.pad(None, Pad::B), None);
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
        let key = |pad| match profile.pad(None, pad) {
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
        let Some(Target::Key(key)) = profile.pad(None, Pad::X) else {
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
        assert_eq!(profile.pad(None, Pad::Select), None);
        assert!(profile.pad(None, Pad::A).is_some());
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
        assert_eq!(profile.pad(None, Pad::A), None);
        assert!(profile.pad(None, Pad::B).is_some());
    }

    /// The point of a layer: the same button means two things, and the one the
    /// layer leaves alone still means what the base says.
    #[test]
    fn a_layer_overrides_what_it_names_and_falls_through_for_the_rest() {
        let profile = resolve(
            r#"
            [pad]
            l2 = "layer:aim"
            a = "Space"
            b = "z"

            [layer.aim.pad]
            a = "Shift"
            "#,
        );
        assert_eq!(profile.pad(None, Pad::L2), Some(&Target::Layer(0)));
        let named = |layer, pad| match profile.pad(layer, pad) {
            Some(Target::Key(key)) => key.key.clone(),
            other => panic!("{pad:?} resolved to {other:?}"),
        };
        assert_eq!(named(None, Pad::A), Key::Character(" ".into()));
        assert_eq!(named(Some(0), Pad::A), Key::Named(NamedKey::Shift));
        // B is the base's in both, which is what makes a layer worth holding.
        assert_eq!(named(Some(0), Pad::B), named(None, Pad::B));
    }

    /// A copy is the only way to add a profile, so it has to carry the whole
    /// mapping over — including what the editor cannot reach.
    #[test]
    fn a_copy_takes_the_bindings_and_the_new_name() {
        let profile = resolve(
            r#"
            name = "Original"
            [pad]
            a = "Space"
            [stick.right]
            analog = "cursor"
            "#,
        );
        let copy = profile.copy("my-game", "My game".to_string(), &KeyNames::new());
        assert_eq!(copy.id, "my-game");
        assert_eq!(copy.name, "My game");
        assert_eq!(copy.pad(None, Pad::A), profile.pad(None, Pad::A));
        assert!(copy.stick(true).is_analog());
        // The copy is the user's, whatever it was copied from.
        assert!(!copy.builtin);
    }

    /// An id collision would shadow a profile instead of adding one, so a name
    /// already spoken for has to land on a stem of its own.
    #[test]
    fn a_typed_name_becomes_a_free_file_stem() {
        let taken = ["vampire-survivors".to_string(), "keys".to_string()];
        assert_eq!(new_id("My Game!", &taken), "my-game");
        assert_eq!(new_id("Vampire Survivors", &taken), "vampire-survivors-2");
        assert_eq!(new_id("  ...  ", &taken), "profile");
    }

    /// A layer that opens a layer is a knot to debug, and a name that is not
    /// there is a typo — both refused rather than half-applied.
    #[test]
    fn an_activator_needs_a_layer_that_exists_and_layers_hold_none() {
        let profile = resolve(
            r#"
            [pad]
            l1 = "layer:nosuch"
            l2 = "layer:aim"

            [layer.aim.pad]
            x = "layer:aim"
            "#,
        );
        assert_eq!(profile.pad(None, Pad::L1), None);
        assert_eq!(profile.pad(None, Pad::L2), Some(&Target::Layer(0)));
        assert_eq!(profile.pad(Some(0), Pad::X), None);
    }
}
