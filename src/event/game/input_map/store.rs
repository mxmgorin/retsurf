//! Where maps live: `input_maps/<id>.toml` files in the user data dir, and the
//! built-ins the binary carries. A file under a built-in's id replaces it, and
//! deleting that file is what "reset to default" means.

use super::raw::RawInputMap;
use super::InputMap;
use crate::config;
use inputbind::sdl::KeyNames;
use std::collections::BTreeMap;

/// Where the per-map files live, under the user data dir.
const MAP_DIR: &str = "input_maps";

impl InputMap {
    /// Write the map to `input_maps/<id>.toml`, which is also how a built-in
    /// is replaced. Returns the re-resolved map, so the edit takes effect
    /// without a restart.
    pub fn save(&self, keys: &KeyNames) -> InputMap {
        let mut saved = InputMap::resolve(&self.id, self.raw.clone(), keys);
        saved.file = write_file(&self.id, &self.raw);
        saved
    }

    /// Remove `input_maps/<id>.toml`. For a built-in that is reset to default —
    /// the binary's own text comes back; for any other map it is deletion.
    pub fn delete(&self) -> bool {
        let path = map_path(&self.id);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                log::info!("input map: removed `{path}`");
                true
            }
            Err(e) => {
                log::warn!("input map: could not remove `{path}`: {e}");
                false
            }
        }
    }
}

/// Where a map of this id is read from and written to.
fn map_path(id: &str) -> String {
    format!("{}{MAP_DIR}/{id}.toml", config::data_dir())
}

/// Write one map's file, reporting whether it is now on disk.
fn write_file(id: &str, raw: &RawInputMap) -> bool {
    let text = match toml::to_string_pretty(raw) {
        Ok(text) => text,
        Err(e) => {
            log::warn!("input map: could not serialize `{id}`: {e}");
            return false;
        }
    };
    let path = map_path(id);
    let _ = std::fs::create_dir_all(format!("{}{MAP_DIR}", config::data_dir()));
    match std::fs::write(&path, text) {
        Ok(()) => {
            log::info!("input map: wrote `{path}`");
            true
        }
        Err(e) => {
            log::warn!("input map: could not write `{path}`: {e}");
            false
        }
    }
}

/// A file stem for a typed name: lowercased, one dash per run of anything
/// else, and never one of `taken` — an id collision would shadow a map
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
        true => "map".to_string(),
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

/// The stock maps, in the order the list shows them. They live in code, so
/// a release that adds one offers it to everyone; a file under the same id
/// replaces it, and deleting that file restores this.
pub(super) const BUILT_IN: [(&str, &str); 4] = [
    ("keys", KEYS_MAP),
    ("wasd", WASD_MAP),
    ("mouse", MOUSE_MAP),
    ("pad", PAD_MAP),
];

/// The retro convention (arrows + z/x/c + Space/Enter) that PICO-8 exports and
/// js13k entries share, so most of itch.io plays with no map edit at all.
const KEYS_MAP: &str = include_str!("../../../../resources/input_maps/keys.toml");

/// The other keyboard convention: WASD and the keys an action game puts round it.
const WASD_MAP: &str = include_str!("../../../../resources/input_maps/wasd.toml");

/// Point and click, for the games the pointer is the whole interface of.
const MOUSE_MAP: &str = include_str!("../../../../resources/input_maps/mouse.toml");

/// The whole pad reaches the page raw — no pointer — for games that read the
/// Gamepad API themselves.
const PAD_MAP: &str = include_str!("../../../../resources/input_maps/pad.toml");

/// Every map this run offers: the built-ins, each replaced by an
/// `input_maps/<id>.toml` that shadows it, plus whatever other files are there.
pub fn load_all(keys: &KeyNames) -> Vec<InputMap> {
    let mut files = read_dir();
    let mut maps: Vec<InputMap> = BUILT_IN
        .iter()
        .map(|(id, text)| {
            let (raw, file) = match files.remove(*id) {
                Some(raw) => (raw, true),
                None => (parse_built_in(id, text), false),
            };
            let mut map = InputMap::resolve(id, raw, keys);
            map.file = file;
            map
        })
        .collect();
    // Whatever else the user put there, in a stable order.
    let mut extra: Vec<(String, RawInputMap)> = files.into_iter().collect();
    extra.sort_by(|(a, _), (b, _)| a.cmp(b));
    maps.extend(extra.into_iter().map(|(id, raw)| {
        let mut map = InputMap::resolve(&id, raw, keys);
        map.file = true;
        map
    }));
    maps
}

/// Whether the binary carries a map of this id.
pub(super) fn is_built_in(id: &str) -> bool {
    BUILT_IN.iter().any(|(built_in, _)| *built_in == id)
}

/// The built-in `id` as the binary carries it — what deleting its file gives
/// back.
pub fn built_in(id: &str, keys: &KeyNames) -> Option<InputMap> {
    BUILT_IN
        .iter()
        .find(|(built_in, _)| *built_in == id)
        .map(|(id, text)| InputMap::resolve(id, parse_built_in(id, text), keys))
}

/// The map `id` names, or the first — the built-in `keys` unless a file
/// shadows it, so an unknown id in the config is never a dead mode.
pub fn pick<'a>(maps: &'a [InputMap], id: &str) -> &'a InputMap {
    maps.iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| maps.first().expect("the built-ins are always offered"))
}

/// A built-in's own text, which a test parses for every one of them.
pub(super) fn parse_built_in(id: &str, text: &str) -> RawInputMap {
    toml::from_str(text).unwrap_or_else(|e| panic!("built-in map `{id}` is invalid: {e}"))
}

/// Read every `input_maps/*.toml`, keyed by file stem. A malformed file is logged
/// and skipped, like a malformed `bindings.toml`.
fn read_dir() -> BTreeMap<String, RawInputMap> {
    let dir = format!("{}{MAP_DIR}", config::data_dir());
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
        match toml::from_str::<RawInputMap>(&text) {
            Ok(raw) => {
                log::info!("input map: loaded `{id}` from `{}`", path.display());
                out.insert(id.to_string(), raw);
            }
            Err(e) => log::error!("input map `{}` is invalid: {e}; ignored", path.display()),
        }
    }
    out
}
