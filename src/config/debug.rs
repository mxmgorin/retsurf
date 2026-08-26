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
    /// Log how long each frame's paint takes, averaged over a second. The one
    /// number that decides whether software rendering is usable on a given
    /// device; see [`crate::app::FrameTimer`].
    pub frame_timing: bool,
}
