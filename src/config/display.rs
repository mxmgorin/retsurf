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
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            use_gles: true,
            cursor_linger_ms: 1500,
            toolbar_position: ToolbarPosition::Top,
            toolbar_autohide: false,
        }
    }
}

/// Which window edge the toolbar sits on. Serializes to `"top"` / `"bottom"`
/// in TOML.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolbarPosition {
    /// At the top of the window, above the page (the default).
    Top,
    /// At the bottom of the window, below the page — handy when the device's
    /// face buttons sit low and a top bar is a reach.
    Bottom,
}

impl ToolbarPosition {
    /// The TOML/UI token for this position (`"top"` / `"bottom"`).
    pub fn as_str(self) -> &'static str {
        match self {
            ToolbarPosition::Top => "top",
            ToolbarPosition::Bottom => "bottom",
        }
    }

    /// Parse leniently: anything that isn't `"bottom"` (case-insensitive) is
    /// `Top`, so a typo can't break the config (mirrors [`CursorMode::from_value`]).
    ///
    /// [`CursorMode::from_value`]: crate::config::CursorMode::from_value
    pub fn from_value(s: &str) -> Self {
        if s.eq_ignore_ascii_case("bottom") {
            ToolbarPosition::Bottom
        } else {
            ToolbarPosition::Top
        }
    }
}

// Deserialize via a string so an unknown value falls back to `Top` instead of
// failing the whole config parse (mirrors `CursorMode`).
impl<'de> Deserialize<'de> for ToolbarPosition {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(&String::deserialize(d)?))
    }
}
