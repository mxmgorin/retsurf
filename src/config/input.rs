use crate::config::token_enum::token_enum;
use crate::config::PadLayout;
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
    /// Delay before the first auto-repeat of navigation, in ms. Applies to
    /// stick-driven OSK navigation and to a held `nav_*` gesture.
    pub osk_nav_initial_delay_ms: u64,
    /// Interval between navigation auto-repeats, in ms — same two uses.
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
    /// Whether pushing the cursor against a window edge scrolls the page that
    /// way. Never in Game Mode, where the cursor belongs to the game.
    pub edge_scroll: bool,
    /// Which pad's letters and colours the on-screen hints draw.
    pub pad_layout: PadLayout,
    /// Read A/B and X/Y across from how the pad reports them, for a pad whose
    /// printed A arrives as B.
    pub swap_face_buttons: bool,
    /// Whether a focused field raises Android's system keyboard. Off leaves
    /// typing to the on-screen keyboard.
    pub system_keyboard: bool,
    /// Whether a page may rumble the pad (the Gamepad API's `playEffect`). Off
    /// also stops advertising the capability to newly loaded documents.
    pub haptics: bool,
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
            edge_scroll: true,
            haptics: true,
            pad_layout: PadLayout::default(),
            swap_face_buttons: false,
            system_keyboard: true,
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

token_enum! {
    /// The default behavior of the D-pad / left stick before any runtime toggle
    /// (see the `scroll` action). Serializes to `"mouse"` / `"scroll"` in TOML;
    /// an unknown value falls back to `Mouse`.
    pub enum CursorMode {
        default Mouse;
        /// Move a clickable on-screen cursor (the default).
        Mouse => "mouse", "Mouse",
        /// Scroll the page.
        Scroll => "scroll", "Scroll",
    }
}
