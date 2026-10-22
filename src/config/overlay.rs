//! Sparse overrides in a config table's own shape: laid over the table to get
//! the effective one, and taken back out of it as the keys that differ.

use toml::Table;

/// Lay `over` onto `base`: a table merges key by key, any other value replaces.
/// A key `base` does not have is logged under `path` and left out, since no
/// field reads it.
fn merge(base: &mut Table, over: &Table, path: &str) {
    for (key, value) in over {
        let at = format!("{path}.{key}");
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(value)) => {
                merge(inner, value, &at)
            }
            (Some(slot), value) => *slot = value.clone(),
            (None, _) => log::warn!("config: override `{at}` names no setting; ignored"),
        }
    }
}

/// The keys of `working` whose values differ from `base`, nested as in both.
fn diff(working: &Table, base: &Table) -> Table {
    let mut out = Table::new();
    for (key, value) in working {
        match (value, base.get(key)) {
            (toml::Value::Table(value), Some(toml::Value::Table(base))) => {
                let inner = diff(value, base);
                if !inner.is_empty() {
                    out.insert(key.clone(), toml::Value::Table(inner));
                }
            }
            (value, Some(base)) if value == base => {}
            (value, _) => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    out
}

/// `base` with `over` laid onto it through their TOML form; `base` again, with
/// a log line, when the result does not read back as a `T`.
pub fn apply<T>(base: &T, over: &Table, path: &str) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned + Clone,
{
    let Ok(mut table) = Table::try_from(base) else {
        return base.clone();
    };
    merge(&mut table, over, path);
    match table.try_into() {
        Ok(value) => value,
        Err(e) => {
            log::warn!("config: overrides of `{path}` are invalid: {e}; ignored");
            base.clone()
        }
    }
}

/// The settings a site's bookmark can override, as dotted paths: a section
/// whole, or one key of it.
pub const SITE_KEYS: &[&str] = &[
    "browser.user_agent",
    "browser.page_theme",
    "browser.page_zoom",
    "adblock.enabled",
    "data_saving",
    "experimental",
    "game_mode",
];

/// The part of `table` under `paths`, nested as in it.
pub fn pick(table: &Table, paths: &[&str]) -> Table {
    let mut out = Table::new();
    for path in paths {
        let (section, key) = match path.split_once('.') {
            Some((section, key)) => (section, Some(key)),
            None => (*path, None),
        };
        let Some(value) = table.get(section) else {
            continue;
        };
        match (key, value) {
            (None, value) => {
                out.insert(section.to_string(), value.clone());
            }
            (Some(key), toml::Value::Table(inner)) => {
                if let Some(value) = inner.get(key) {
                    let slot = out
                        .entry(section.to_string())
                        .or_insert_with(|| toml::Value::Table(Table::new()));
                    if let toml::Value::Table(slot) = slot {
                        slot.insert(key.to_string(), value.clone());
                    }
                }
            }
            (Some(_), _) => {}
        }
    }
    out
}

/// The [`SITE_KEYS`] part of `config`, in its TOML form.
pub fn site_part<T: serde::Serialize>(config: &T) -> Table {
    Table::try_from(config).map_or_else(|_| Table::new(), |table| pick(&table, SITE_KEYS))
}

/// The keys of `working` that differ from `base`, as a table.
pub fn changes<T: serde::Serialize>(working: &T, base: &T) -> Table {
    match (Table::try_from(working), Table::try_from(base)) {
        (Ok(working), Ok(base)) => diff(&working, &base),
        _ => Table::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GameModeConfig, Scaling};

    #[test]
    fn an_override_lands_and_reads_back_as_its_diff() {
        let base = GameModeConfig::default();
        let over: Table =
            toml::from_str("input_map = \"wasd\"\n[view]\nscaling = \"fit\"\nnot_a_key = 1\n")
                .unwrap();
        let working = apply(&base, &over, "game_mode");
        assert_eq!(working.input_map, "wasd");
        assert_eq!(working.view.scaling, Scaling::Fit);
        assert_eq!(working.view.cursor, base.view.cursor);
        let back = changes(&working, &base);
        let expected: Table =
            toml::from_str("input_map = \"wasd\"\n[view]\nscaling = \"fit\"\n").unwrap();
        assert_eq!(back, expected);
        assert!(changes(&base, &base).is_empty());
    }

    #[test]
    fn pick_takes_whole_sections_and_single_keys() {
        let table: Table = toml::from_str(
            "[browser]\nuser_agent = \"mobile\"\nrestore_tabs = true\n\
             [game_mode]\ninput_map = \"keys\"\n[interface]\nscale = 1.0\n",
        )
        .unwrap();
        let picked = pick(
            &table,
            &["browser.user_agent", "browser.page_zoom", "game_mode"],
        );
        let expected: Table = toml::from_str(
            "[browser]\nuser_agent = \"mobile\"\n[game_mode]\ninput_map = \"keys\"\n",
        )
        .unwrap();
        assert_eq!(picked, expected);
    }

    /// The site part of a config carries every site key.
    #[test]
    fn the_site_part_covers_every_site_key() {
        let part = site_part(&crate::config::AppConfig::default());
        for path in SITE_KEYS {
            let (section, key) = path
                .split_once('.')
                .map_or((*path, None), |(s, k)| (s, Some(k)));
            let inner = part.get(section).and_then(|v| v.as_table());
            assert!(inner.is_some(), "{path}");
            if let Some(key) = key {
                assert!(inner.is_some_and(|t| t.contains_key(key)), "{path}");
            }
        }
    }

    #[test]
    fn a_bad_override_leaves_the_base() {
        let base = GameModeConfig::default();
        let over: Table = toml::from_str("patches = \"yes\"\n").unwrap();
        assert_eq!(apply(&base, &over, "game_mode").patches, base.patches);
    }
}
