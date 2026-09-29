use serde::{Deserialize, Serialize};

/// The window and how it is drawn (`[display]` in the config): its size, the
/// GL backend, and panel quirks.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// The size the window opens at, where the driver leaves that to us (desktop,
    /// never a handheld). Rewritten on exit with the size it was left at.
    pub width: u32,
    pub height: u32,
    /// Request an OpenGL ES context (required on Mali handhelds) instead of
    /// desktop GL. Can be overridden at startup via `RETSURF_GLES=0`.
    pub use_gles: bool,
    /// Render the page and the chrome on the CPU, with no GL context at all —
    /// the only thing that draws on a GPU-less device (Miyoo Mini). Needs the
    /// `software` build feature; overridden at startup via `RETSURF_SOFTWARE=1`.
    pub software_render: bool,
    /// Paint the screen's last row black. Some panels show that row again as the
    /// first one, so a light page bleeds a band above the toolbar (muOS/A133).
    pub dark_last_row: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            use_gles: true,
            software_render: false,
            dark_last_row: false,
        }
    }
}
