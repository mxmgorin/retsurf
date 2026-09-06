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
    /// The same report, written to the log every ten seconds and drawn nowhere.
    /// What the overlay cannot do on a handheld: it covers the screen it is
    /// measuring, and the answer is wanted over ssh anyway.
    pub memory_log: bool,
    /// Log how long each frame's paint takes, averaged over a second. The one
    /// number that decides whether software rendering is usable on a given
    /// device; see [`crate::app::FrameTimer`].
    pub frame_timing: bool,
    /// CPU and major faults per thread family, plus run totals at exit — what
    /// `frame_timing` cannot say: work done, or waiting. See
    /// [`crate::platform::threads`].
    pub thread_cpu: bool,
}
