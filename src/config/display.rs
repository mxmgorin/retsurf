use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// Window/display settings (`[display]` in the config): size, GL backend, and
/// cursor-visibility timing.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub width: u32,
    pub height: u32,
    /// Request an OpenGL ES context (required on Mali handhelds) instead of
    /// desktop GL. Can be overridden at startup via `RETSURF_GLES=0`.
    pub use_gles: bool,
    /// Render the page and the chrome on the CPU, with no GL context at all —
    /// the only thing that draws on a GPU-less device (Miyoo Mini). Needs the
    /// `software` build feature; overridden at startup via `RETSURF_SOFTWARE=1`.
    pub software_render: bool,
    /// Frames per second the software renderer is held to (`0` uncapped,
    /// `RETSURF_MAX_FPS` overrides). Nothing else paces it, and the cap paces
    /// the whole loop — including how often Servo's callbacks are pumped.
    pub max_fps: u32,
    /// How long the virtual cursor stays visible after the last movement, in ms.
    /// It hides when idle (nothing to hover) but lingers so you can see where it
    /// landed before clicking.
    pub cursor_linger_ms: u64,
    /// Which edge the toolbar (address bar + nav buttons) sits on.
    pub toolbar_position: ToolbarPosition,
    /// Hide the toolbar while scrolling down, reveal it on scrolling up. A top
    /// toolbar reserves space when shown (the page reflows below it, so the bar
    /// never covers content); a bottom toolbar floats over the page and slides away.
    pub toolbar_autohide: bool,
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
            max_fps: 30,
            cursor_linger_ms: 1500,
            toolbar_position: ToolbarPosition::Top,
            toolbar_autohide: false,
            dark_last_row: false,
        }
    }
}

token_enum! {
    /// Which window edge the toolbar sits on. Serializes to `"top"` / `"bottom"`
    /// in TOML; an unknown value falls back to `Top`.
    pub enum ToolbarPosition {
        default Top;
        /// At the top of the window, above the page (the default).
        Top => "top", "Top",
        /// At the bottom of the window, below the page — handy when the device's
        /// face buttons sit low and a top bar is a reach.
        Bottom => "bottom", "Bottom",
    }
}
