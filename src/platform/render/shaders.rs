//! Page shaders, single-pass RetroArch GLSL: the built-ins from
//! `resources/shaders` and the user's `shaders/<id>.glsl` in the data dir,
//! which shadow a built-in of the same id. The internal passes around them
//! compile behind [`HEADER`].

use crate::config;
use std::borrow::Cow;

/// The id that draws the page without a shader.
pub const NONE: &str = "none";

pub const VERTEX: &str = include_str!("assets/page.vert");
pub const HEADER: &str = include_str!("assets/header.frag");
/// The page unchanged: the backdrop around the game, and the stand-in for a
/// shader that failed to build.
pub const PLAIN: &str = include_str!("assets/plain.frag");
/// Samples the game out of the page at its own resolution.
pub const EXTRACT: &str = include_str!("assets/extract.frag");

const DIR: &str = "shaders";
const EXTENSION: &str = "glsl";

/// A shader compiled into the binary.
struct BuiltIn {
    id: &'static str,
    label: &'static str,
    source: &'static str,
    /// Samples the game bilinearly, as its RetroArch preset asks.
    linear: bool,
}

const BUILT_INS: &[BuiltIn] = &[
    BuiltIn {
        id: "scanlines",
        label: "Scanlines",
        source: include_str!("../../../resources/shaders/scanlines.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "lcd-grid",
        label: "LCD grid",
        source: include_str!("../../../resources/shaders/lcd-grid.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "zfast-crt",
        label: "zfast CRT",
        source: include_str!("../../../resources/shaders/zfast-crt.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "crt-pi",
        label: "CRT Pi",
        source: include_str!("../../../resources/shaders/crt-pi.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "crt-lottes",
        label: "CRT Lottes",
        source: include_str!("../../../resources/shaders/crt-lottes.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "xbr-lv3",
        label: "xBR",
        source: include_str!("../../../resources/shaders/xbr-lv3.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "omniscale",
        label: "OmniScale",
        source: include_str!("../../../resources/shaders/omniscale.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "sharp-bilinear",
        label: "Sharp bilinear",
        source: include_str!("../../../resources/shaders/sharp-bilinear.glsl"),
        linear: true,
    },
    BuiltIn {
        id: "sameboy-lcd",
        label: "SameBoy LCD",
        source: include_str!("../../../resources/shaders/sameboy-lcd.glsl"),
        linear: false,
    },
    BuiltIn {
        id: "sameboy-mono-lcd",
        label: "SameBoy mono LCD",
        source: include_str!("../../../resources/shaders/sameboy-mono-lcd.glsl"),
        linear: false,
    },
];

/// Screen rows the whole page counts as when the game's own size is unknown.
const IMITATED_ROWS: f32 = 240.0;
/// The narrowest guessed game pixel, so a shader still has rows to work with.
const MIN_PERIOD: f32 = 2.0;

/// Every pickable shader as `(label, id)`: [`NONE`], the built-ins, then the
/// user's by id. Reads the shader folder.
pub fn list() -> Vec<(String, String)> {
    let mut out = vec![("None".to_string(), NONE.to_string())];
    out.extend(
        BUILT_INS
            .iter()
            .map(|b| (b.label.to_string(), b.id.to_string())),
    );
    let user = user_ids()
        .into_iter()
        .filter(|id| id != NONE && builtin(id).is_none());
    out.extend(user.map(|id| (id.clone(), id)));
    out
}

/// How `id` reads in a row: a built-in's label, else the id itself.
pub fn label(id: &str) -> String {
    match builtin(id) {
        Some(b) => b.label.to_string(),
        None if id == NONE || id.is_empty() => "None".to_string(),
        None => id.to_string(),
    }
}

/// Whether `id` names a shader that can be drawn, or [`NONE`].
pub fn exists(id: &str) -> bool {
    id == NONE || source(id).is_some()
}

/// A shader's text and how it samples the game.
pub struct Source {
    pub text: Cow<'static, str>,
    /// Bilinear rather than nearest; a user's file is always nearest.
    pub linear: bool,
}

/// `id`'s source, the user's file before a built-in; `None` for [`NONE`] and
/// for an id with neither.
pub fn source(id: &str) -> Option<Source> {
    // An id names a file in the folder, never a path out of it.
    if id == NONE || id.is_empty() || id.contains(['/', '\\']) {
        return None;
    }
    match std::fs::read_to_string(user_path(id)) {
        Ok(text) => Some(Source {
            text: Cow::Owned(text),
            linear: false,
        }),
        Err(_) => builtin(id).map(|b| Source {
            text: Cow::Borrowed(b.source),
            linear: b.linear,
        }),
    }
}

/// Output pixels per game pixel when only the page's `height_px` is known.
pub fn guessed_period(height_px: f32) -> f32 {
    (height_px / IMITATED_ROWS).round().max(MIN_PERIOD)
}

fn builtin(id: &str) -> Option<&'static BuiltIn> {
    BUILT_INS.iter().find(|b| b.id == id)
}

fn user_path(id: &str) -> String {
    format!("{}{DIR}/{id}.{EXTENSION}", config::data_dir())
}

/// The `.glsl` stems in the shader folder, sorted.
fn user_ids() -> Vec<String> {
    let dir = format!("{}{DIR}", config::data_dir());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == EXTENSION))
        .filter_map(|p| p.file_stem()?.to_str().map(str::to_string))
        .collect();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_and_every_built_in_are_listed_once() {
        let ids: Vec<String> = list().into_iter().map(|(_, id)| id).collect();
        assert_eq!(ids[0], NONE);
        for b in BUILT_INS {
            assert_eq!(ids.iter().filter(|id| *id == b.id).count(), 1, "{}", b.id);
        }
    }

    #[test]
    fn an_unknown_id_reads_as_itself_and_has_no_source() {
        assert_eq!(label("my-crt"), "my-crt");
        assert_eq!(label(NONE), "None");
        assert!(source(NONE).is_none());
        assert!(source("../config").is_none());
    }

    #[test]
    fn a_guessed_period_never_drops_below_two() {
        assert_eq!(guessed_period(100.0), MIN_PERIOD);
        assert_eq!(guessed_period(1440.0), 6.0);
    }
}
