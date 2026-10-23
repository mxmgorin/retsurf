//! JS engine presets: named sets of SpiderMonkey JIT switches and a heap scale,
//! picked by `[performance] js_preset` globally or per site. The built-ins come
//! first; a `[js_presets.<id>]` table overrides a built-in key by key or adds a
//! preset of its own. A key left out keeps the memory profile's choice.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The preset that changes nothing: the memory profile's choice throughout.
pub const AUTO: &str = "auto";

/// One preset; `None` keeps the memory profile's value.
#[derive(Clone, Copy, Default, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct JsPreset {
    /// The optimizing JIT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ion: Option<bool>,
    /// The optimizing wasm tier, which asm.js also compiles through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_ion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_jit: Option<bool>,
    /// Compile on helper threads instead of the page's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offthread_compilation: Option<bool>,
    /// The profile's JS heap ceiling times this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap: Option<f32>,
}

impl JsPreset {
    /// `self` with `over`'s keys laid on it.
    fn with(self, over: &JsPreset) -> JsPreset {
        JsPreset {
            ion: over.ion.or(self.ion),
            wasm_ion: over.wasm_ion.or(self.wasm_ion),
            baseline_jit: over.baseline_jit.or(self.baseline_jit),
            offthread_compilation: over.offthread_compilation.or(self.offthread_compilation),
            heap: over.heap.or(self.heap),
        }
    }
}

/// The built-in presets, in the order they are offered.
const BUILT_IN: &[(&str, &str, JsPreset)] = &[
    (
        AUTO,
        "Auto",
        JsPreset {
            ion: None,
            wasm_ion: None,
            baseline_jit: None,
            offthread_compilation: None,
            heap: None,
        },
    ),
    (
        "fast",
        "Fast",
        JsPreset {
            ion: Some(true),
            wasm_ion: Some(true),
            baseline_jit: Some(true),
            offthread_compilation: Some(true),
            heap: Some(2.0),
        },
    ),
    (
        "lean",
        "Lean",
        JsPreset {
            ion: Some(false),
            wasm_ion: Some(false),
            baseline_jit: Some(true),
            offthread_compilation: None,
            heap: Some(0.5),
        },
    ),
    (
        "safe",
        "Safe",
        JsPreset {
            ion: Some(false),
            wasm_ion: Some(false),
            baseline_jit: Some(false),
            offthread_compilation: None,
            heap: None,
        },
    ),
];

/// Every preset as `(id, preset)`: the built-ins with the user's keys on them,
/// then the user's own by id.
pub fn all(user: &BTreeMap<String, JsPreset>) -> Vec<(String, JsPreset)> {
    let built_in = BUILT_IN.iter().map(|(id, _, preset)| {
        let preset = user.get(*id).map_or(*preset, |over| preset.with(over));
        (id.to_string(), preset)
    });
    let own = user
        .iter()
        .filter(|(id, _)| !is_built_in(id))
        .map(|(id, preset)| (id.clone(), *preset));
    built_in.chain(own).collect()
}

/// Preset `id`, or [`AUTO`]'s when there is none of that id.
pub fn get(user: &BTreeMap<String, JsPreset>, id: &str) -> JsPreset {
    all(user)
        .into_iter()
        .find(|(have, _)| have == id)
        .map_or_else(JsPreset::default, |(_, preset)| preset)
}

/// Whether `id` names a preset.
pub fn exists(user: &BTreeMap<String, JsPreset>, id: &str) -> bool {
    is_built_in(id) || user.contains_key(id)
}

fn is_built_in(id: &str) -> bool {
    BUILT_IN.iter().any(|(have, _, _)| *have == id)
}

/// How preset `id` is shown: a built-in's name, else the id itself.
pub fn label(id: &str) -> String {
    BUILT_IN
        .iter()
        .find(|(have, _, _)| *have == id)
        .map_or_else(|| id.to_string(), |(_, label, _)| label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_users_table_overrides_a_built_in_and_adds_its_own() {
        let user: BTreeMap<String, JsPreset> =
            toml::from_str("[fast]\nheap = 3.0\n[ftl]\nion = true\nwasm_ion = true\n").unwrap();
        let ids: Vec<String> = all(&user).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["auto", "fast", "lean", "safe", "ftl"]);
        let fast = get(&user, "fast");
        assert_eq!(fast.heap, Some(3.0));
        assert_eq!(fast.ion, Some(true));
        assert_eq!(get(&user, "ftl").baseline_jit, None);
        assert_eq!(get(&user, "nope"), JsPreset::default());
        assert!(exists(&user, "ftl") && exists(&user, "safe") && !exists(&user, "nope"));
        assert_eq!(label("fast"), "Fast");
        assert_eq!(label("ftl"), "ftl");
    }
}
