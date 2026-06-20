use serde::{Deserialize, Serialize};

/// Tunables for the gamepad-driven cursor, scroll, and on-screen-keyboard input,
/// plus the button bindings (see [`crate::event::bindings`]).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Stick deflection below this (normalized 0..1) is treated as centered.
    pub deadzone: f32,
    /// Cursor speed at full stick deflection, logical px per second.
    pub cursor_speed: f32,
    /// Scroll speed at full stick deflection, device px per second.
    pub scroll_speed: f32,
    /// Trigger pull (normalized) above which L2/R2 count as pressed.
    pub trigger_threshold: f32,
    /// Stick deflection above which it counts as a directional OSK press.
    pub osk_nav_threshold: f32,
    /// Delay before the first auto-repeat of stick-driven OSK navigation, in ms.
    pub osk_nav_initial_delay_ms: u64,
    /// Interval between auto-repeats of stick-driven OSK navigation, in ms.
    pub osk_nav_repeat_ms: u64,
    /// Holding a bound button this long fires its `hold:` gesture. The
    /// bindings themselves live in `bindings.toml` — see
    /// [`crate::event::bindings`].
    pub hold_ms: u64,
    /// Default D-pad/stick mode at startup ([`CursorMode`]). Toggle live with the
    /// `scroll` action; this only sets the initial mode.
    pub cursor_mode: CursorMode,
    /// Whether link-hint mode shows typed combo badges and routes the gamepad
    /// buttons as combo symbols. Off restores plain spatial hopping: the D-pad
    /// (and stick) hop the selection again and the buttons keep their normal
    /// meaning. See [`crate::overlay::hints`].
    pub hint_badges: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            deadzone: 0.25,
            cursor_speed: 600.0,
            scroll_speed: 1600.0,
            trigger_threshold: 0.5,
            osk_nav_threshold: 0.5,
            osk_nav_initial_delay_ms: 350,
            osk_nav_repeat_ms: 140,
            hold_ms: 400,
            cursor_mode: CursorMode::Mouse,
            hint_badges: true,
        }
    }
}

impl InputConfig {
    /// Whether the gamepad should start in scroll mode (vs the default cursor),
    /// per [`cursor_mode`](Self::cursor_mode).
    pub fn starts_in_scroll_mode(&self) -> bool {
        self.cursor_mode == CursorMode::Scroll
    }
}

/// The default behavior of the D-pad / left stick before any runtime toggle
/// (see the `scroll` action). Serializes to `"mouse"` / `"scroll"` in TOML.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorMode {
    /// Move a clickable on-screen cursor (the default).
    Mouse,
    /// Scroll the page.
    Scroll,
}

impl CursorMode {
    /// The TOML/UI token for this mode (`"mouse"` / `"scroll"`).
    pub fn as_str(self) -> &'static str {
        match self {
            CursorMode::Mouse => "mouse",
            CursorMode::Scroll => "scroll",
        }
    }

    /// Parse leniently: anything that isn't `"scroll"` (case-insensitive) is
    /// `Mouse`, so a typo can't break the config (mirrors `sanitize`'s clamping).
    pub fn from_value(s: &str) -> Self {
        if s.eq_ignore_ascii_case("scroll") {
            CursorMode::Scroll
        } else {
            CursorMode::Mouse
        }
    }
}

// Deserialize via a string so an unknown value falls back to `Mouse` instead of
// failing the whole config parse — the rest of the config degrades gracefully too.
impl<'de> Deserialize<'de> for CursorMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(&String::deserialize(d)?))
    }
}
