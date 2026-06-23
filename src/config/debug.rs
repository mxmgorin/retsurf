use serde::{Deserialize, Serialize};

/// Developer/diagnostic toggles (`[debug]` in the config). Off by default and
/// not surfaced in the Settings GUI — these are hand-edited in `retsurf.toml`
/// for on-device profiling.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    /// Overlay Servo's live memory report (its `about:memory` data) in a corner,
    /// rolled up by subsystem (image-cache, layout, JS, ...). For measuring where
    /// RAM goes on a target device. See [`crate::ui`]'s memory overlay.
    pub memory_overlay: bool,
}
