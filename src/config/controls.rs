use crate::config::token_enum::token_enum;
use crate::config::PadLayout;
use serde::{Deserialize, Serialize};

/// Tunables for the gamepad-driven cursor, scroll, and on-screen-keyboard input
/// (`[controls]` in the config). The button bindings live in their own file
/// (see [`crate::event::bindings`]).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlsConfig {
    /// Stick deflection below this (normalized 0..1) is treated as centered.
    pub deadzone: f32,
    /// Cursor speed at full stick deflection, logical px per second.
    pub cursor_speed: f32,
    /// Exponent of the stick-to-cursor response past the dead zone: above 1, a
    /// small push moves slowly and a full one at `cursor_speed`.
    pub cursor_curve: f32,
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
    /// What the D-pad does on a page; overlays keep it as their own.
    pub dpad: StickRole,
    /// What the left stick does on a page; overlays keep it as their own.
    pub left_stick: StickRole,
    /// What the right stick does on a page; the on-screen keyboard keeps it.
    pub right_stick: StickRole,
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

impl Default for ControlsConfig {
    fn default() -> Self {
        Self {
            deadzone: 0.25,
            cursor_speed: 600.0,
            cursor_curve: crate::config::bounds::CURSOR_CURVE.default as f32,
            scroll_speed: 1600.0,
            trigger_threshold: 0.5,
            osk_nav_threshold: 0.5,
            osk_nav_initial_delay_ms: 350,
            osk_nav_repeat_ms: 140,
            hold_ms: 400,
            dpad: StickRole::Cursor,
            left_stick: StickRole::Cursor,
            right_stick: StickRole::Scroll,
            hint_badges: true,
            edge_scroll: true,
            haptics: true,
            pad_layout: PadLayout::default(),
            swap_face_buttons: false,
            system_keyboard: true,
        }
    }
}

token_enum! {
    /// What a stick or the D-pad does on a page.
    pub enum StickRole {
        default Cursor;
        /// Move the cursor.
        Cursor => "cursor", "Cursor",
        /// Scroll the page.
        Scroll => "scroll", "Scroll",
        None => "none", "None",
    }
}
